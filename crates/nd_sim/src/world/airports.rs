//! [airport] Airports, the systems (`crate::airport` holds the data, the
//! layout and the rules): fitting one to the ground and placing it, its
//! lot and forecourt road, the timetable and the planes, passengers to and
//! from town, air cargo, the fees and upkeep, noise on the flight paths and
//! the complaints it brings, and the inspector's report.

use super::market::TruckOrigin;
use super::*;
use crate::airport::{self as ap, AirLoad, Airport, AirportId, AirportSize, Plane, PlaneKind, PlanePhase};
use crate::tourism::{self as tm, Arrival, Goal, Tourist, TouristState};

/// Salt for the airport's hashes (no world RNG draws).
const SALT: u64 = 0xa1_77_0b_7e;

/// Headings tried when fitting an airport to the ground, degrees.
const FIT_STEP: f32 = 5.0;

/// [airport] What the inspector and the remote API show of an airport.
#[derive(Clone, Debug, Serialize)]
pub struct AirportReport {
    pub name: String,
    pub size: &'static str,
    pub works: bool,
    /// Why it does not, when it does not.
    pub problem: Option<String>,
    pub runways: usize,
    pub stands: usize,
    pub heading_deg: f32,
    pub landing_towards_deg: f32,
    /// One end closed by hills: it lands one way and takes off the other.
    pub one_way: bool,
    pub flights_today: u32,
    pub flights_yesterday: u32,
    pub scheduled_today: usize,
    pub pax_today: u32,
    pub pax_yesterday: u32,
    pub tourists_in_today: u32,
    pub business_today: u32,
    pub residents_out_today: u32,
    pub cargo_in_today: f32,
    pub cargo_out_today: f32,
    pub cargo_waiting: f32,
    pub delayed_today: u32,
    pub avg_delay_min: f32,
    pub cancelled_today: u32,
    pub complaints_yesterday: u32,
    pub homes_in_noise: u32,
    pub fees_today: f32,
    pub fees_yesterday: f32,
    pub upkeep_per_day: f32,
    pub jobs: u16,
    pub workers: u16,
    pub demand_pax_per_day: f32,
    pub access: Vec<(&'static str, u32)>,
    pub transit: Vec<String>,
    pub planes: Vec<PlaneRow>,
    pub next_arrivals: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlaneRow {
    pub flight: String,
    pub kind: &'static str,
    pub phase: &'static str,
    pub from: &'static str,
    pub to: &'static str,
    pub stand: u8,
    pub late_min: u32,
}

impl World {
    // ── placing ────────────────────────────────────────────────────────────

    /// [airport] Where an airport of `size` would go for a click at `p`:
    /// at the heading given (degrees of its runways from +x), or, without
    /// one, at whichever heading fits the ground there best (least
    /// earthwork and fewest buildings bought out) and clears everything.
    /// Err says why not (for the best heading tried).
    pub fn plan_airport(&self, size: AirportSize, p: Vec2, heading_deg: Option<f32>) -> Result<Airport, String> {
        self.plan_airport_with(size, p, heading_deg, false)
    }

    /// [airport] [`World::plan_airport`]; with `clear`, the streets across
    /// the site are taken out for it (paid for by the metre) instead of
    /// refusing it. Highways, railways, bridges and civic buildings still
    /// refuse it.
    pub fn plan_airport_with(&self, size: AirportSize, p: Vec2, heading_deg: Option<f32>, clear: bool) -> Result<Airport, String> {
        if let Some(why) = self.airport_count_problem(size) {
            return Err(why);
        }
        let headings: Vec<f32> = match heading_deg {
            Some(h) => vec![h],
            None => (0..(180.0 / FIT_STEP) as usize).map(|i| i as f32 * FIT_STEP).collect(),
        };
        let mut best: Option<(f32, Airport)> = None;
        let mut why: Option<(usize, f32, String)> = None;
        for h in headings {
            let along = Vec2::new(h.to_radians().cos(), h.to_radians().sin());
            let a = Airport::new(size, p, along, 0.0, String::new());
            match self.airport_site(&a, clear) {
                Ok((level, score, earthwork, street_m, one_way)) => {
                    if best.as_ref().is_none_or(|(s, _)| score < *s) {
                        let mut a = a;
                        a.level = level;
                        a.earthwork_cost = earthwork + street_m * ap::CLEAR_COST_PER_M;
                        a.cleared_m = street_m;
                        a.one_way = one_way;
                        best = Some((score, a));
                    }
                }
                // The furthest any heading got, and of those the nearest miss.
                Err((stage, bad, e)) => {
                    if why.as_ref().is_none_or(|(s, b, _)| stage > *s || (stage == *s && bad < *b)) {
                        why = Some((stage, bad, e));
                    }
                }
            }
        }
        match best {
            Some((_, a)) if self.economy.treasury < size.cost() + a.earthwork_cost => Err(format!(
                "the treasury has {:.0}; {} costs {:.0} here ({:.0} and {:.0} of earthworks)",
                self.economy.treasury,
                crate::names::a(size.label()),
                size.cost() + a.earthwork_cost,
                size.cost(),
                a.earthwork_cost
            )),
            Some((_, a)) => Ok(a),
            None => Err(why.map(|w| w.2).unwrap_or_else(|| "no room for it here".into())),
        }
    }

    /// [airport] At most one airport of each size (the airfield and the
    /// international airport may stand together).
    fn airport_count_problem(&self, size: AirportSize) -> Option<String> {
        self.airports.airports.values().find(|a| a.size == size).map(|a| format!("the city has its {} already ({}): one of each size per city", size.label(), a.name))
    }

    /// Whether an airport planned as `a` can stand there: the levelled
    /// ground's height and a score (lower is better), or how far the
    /// checks got and why not (a later stage is the more useful reason).
    fn airport_site(&self, a: &Airport, clear: bool) -> Result<(f32, f32, f32, f32, Option<f32>), (usize, f32, String)> {
        let fp = a.footprint();
        let edge = crate::landscape::MAP_EDGE - 10.0;
        if fp.iter().any(|c| c.x.abs() > edge || c.y.abs() > edge) {
            return Err((0, 0.0, format!("{} needs about {:.0} × {:.0} m of ground: it runs off the map here", crate::names::a(a.size.label()), a.size.size().0, a.size.size().1)));
        }
        // Dry ground, level enough to grade: samples over the whole site.
        let (w, d) = a.size.size();
        let land = &self.landscape;
        let mut heights = Vec::new();
        let step = 40.0;
        let (nx, ny) = ((w / step).ceil() as i32, (d / step).ceil() as i32);
        for i in 0..=nx {
            for j in 0..=ny {
                let q = a.world(Vec2::new(-w * 0.5 + w * i as f32 / nx as f32, -d * 0.5 + d * j as f32 / ny as f32));
                // A stream is put in a culvert; a river, lake or the sea is too much.
                if !land.is_flat() && land.water_at(q).is_some_and(|h| h.width > ap::CULVERT_WIDTH) {
                    return Err((1, 0.0, format!("the site runs into the water at ({:.0}, {:.0}): an airport needs dry ground (a stream it can culvert)", q.x, q.y)));
                }
                heights.push(land.height(q));
            }
        }
        // The flat basin is drawn and built at one level (no relief): nothing to grade.
        if land.is_flat() {
            heights.iter_mut().for_each(|h| *h = 0.0);
        }
        // Levelled to the middle of its highest and lowest ground: the least
        // cut or fill at the worst place.
        let (lo, hi) = heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(*h), hi.max(*h)));
        let level = if land.is_flat() { 0.0 } else { ((lo + hi) * 0.5).max(crate::landscape::BANK_HEIGHT) };
        let worst = heights.iter().map(|h| (h - level).abs()).fold(0.0, f32::max);
        if worst > ap::MAX_EARTHWORK {
            return Err((2, worst, format!("too hilly: levelling the site would cut or fill {worst:.0} m (at most {:.0}); try flatter ground or another heading", ap::MAX_EARTHWORK)));
        }
        let earthwork = heights.iter().map(|h| (h - level).abs()).sum::<f32>() / heights.len().max(1) as f32;
        // Nothing over a road; off every station, yard, dock and airport.
        let mut street_m = 0.0;
        if clear {
            match self.airport_ways_under(a) {
                Err(why) => return Err((3, 0.0, why)),
                Ok(m) => street_m = m,
            }
        } else if let Some(why) = self.road_under(crate::world::over_roads::Footprint::Area(&fp), a.size.label(), true) {
            return Err((3, 0.0, format!("{why} (or clear the streets for it: shift-click, or `clear: true`)")));
        }
        if self.facilities.values().any(|f| polygons_overlap(&f.footprint(), &fp)) || self.ports.ports.values().any(|o| polygons_overlap(&o.footprint(), &fp)) || self.airports.airports.values().any(|o| polygons_overlap(&o.footprint(), &fp)) {
            return Err((3, 0.0, "it would overlap a station, yard, dock or another airport".into()));
        }
        if !self.pillars.is_empty() && self.pillars_on(&fp) {
            return Err((3, 0.0, "a column of a raised road stands on the site".into()));
        }
        let under = self.buildings_under(&fp);
        if let Some(b) = under.iter().filter_map(|(id, _)| self.buildings.get(*id)).find(|b| Self::is_civic_structure(b)) {
            return Err((4, 0.0, format!("{} stands on the site; bulldoze it first", b.name)));
        }
        // The approaches off both ends of every runway: no hill and no
        // building rising through the approach surface.
        // Hills must stay under the glide path with room to spare
        // (buildings are held to the approach surface below; the ground just
        // off the ends is graded with the field). An end with a hill in the
        // way is closed: planes land towards it and take off away from it,
        // as at a mountain airfield. Both closed, no airport.
        let lay = a.layout();
        let mut closed: Option<(f32, f32, String)> = None;
        let mut open = Vec::new();
        for s in [-1.0f32, 1.0] {
            let mut blocked: Option<(f32, String)> = None;
            for r in &lay.runways {
                let mut out = 0.0;
                while out <= ap::APPROACH_TERRAIN && blocked.is_none() && !land.is_flat() {
                    for off in [-0.8f32, 0.0, 0.8] {
                        let half = (if a.size == AirportSize::International { 150.0 } else { 75.0 }) + out * ap::APPROACH_SPLAY;
                        let q = a.world(Vec2::new(s * (r.length * 0.5 + ap::APPROACH_START + out), r.y + off * half));
                        if q.x.abs() > crate::landscape::LAND_HALF || q.y.abs() > crate::landscape::LAND_HALF {
                            continue;
                        }
                        let lim = ap::terrain_limit(out);
                        let rise = land.height(q) - level;
                        if rise > lim && blocked.is_none() {
                            blocked = Some((rise - lim, format!("a hill rises {:.0} m too close under the glide path {:.0} m off the runway's {} end (at most {:.0} m there)", rise - lim, out, end_name(a.along * s), lim)));
                        }
                    }
                    out += 100.0;
                }
            }
            match blocked {
                Some((by, why)) => closed = Some((s, by, why)),
                None => open.push(s),
            }
        }
        let one_way = match (&closed, open.as_slice()) {
            (None, _) => None,
            // The approach comes in over the open end: landing towards the closed one.
            (Some(_), [s_open]) => Some(-*s_open),
            (Some((_, by, why)), _) => return Err((5, *by, format!("{why}, and at the other end too: turn the runway or find lower ground"))),
        };
        // What rises through the approach surfaces: refused, or with
        // `clear` bought out and pulled down (not the city's own buildings).
        let (tall, why) = self.approach_obstacles(a, level, &under);
        if let Some(why) = why.filter(|_| !clear || tall.is_empty()) {
            return Err((6, 0.0, why));
        }
        if !clear && !tall.is_empty() {
            return Err((6, 0.0, "tall buildings in the approach path".into()));
        }
        let street_m = street_m + tall.len() as f32 * ap::OBSTACLE_COST / ap::CLEAR_COST_PER_M;
        // Grown buildings on the site are bought out: a few count against it.
        // Two-way operation is worth some earthwork.
        let score = earthwork + under.len() as f32 * 0.4 + street_m * 0.01 + if one_way.is_some() { 6.0 } else { 0.0 };
        let cost = earthwork * w * d * ap::EARTHWORK_COST;
        Ok((level, score, cost, street_m, one_way))
    }

    /// Buildings off the site rising through the approach surfaces of `a`
    /// (levelled to `level`) that may be bought out, and why the site is
    /// refused when one of them is the city's own (or, for the message, the
    /// first tall one).
    fn approach_obstacles(&self, a: &Airport, level: f32, under: &[(BuildingId, LotId)]) -> (Vec<(BuildingId, LotId)>, Option<String>) {
        let fp_ids: std::collections::HashSet<BuildingId> = under.iter().map(|(id, _)| *id).collect();
        let mut tall = Vec::new();
        let mut why = None;
        for (id, b) in self.buildings.iter() {
            if fp_ids.contains(&id) || b.floors == 0 {
                continue;
            }
            let Some(l) = self.lots.get(b.lot) else { continue };
            let Some(lim) = a.approach_limit(l.center) else { continue };
            let top = self.lot_pad(b.lot) - level + b.floors as f32 * crate::towers::FLOOR_M[l.land_use.index()];
            if top > lim + 0.5 {
                let msg = format!("{} ({} floors) stands in the approach path, rising {:.0} m above the {:.0} m limit there: turn the runway, build elsewhere, or clear it (shift-click, `clear: true`)", b.name, b.floors, top - lim, lim.max(0.0));
                if Self::is_civic_structure(b) {
                    return (tall, Some(format!("{} (a city building: bulldoze it first)", msg.split(':').next().unwrap_or(&msg))));
                }
                if why.is_none() {
                    why = Some(msg);
                }
                tall.push((id, b.lot));
            }
        }
        (tall, why)
    }

    /// Whoever was left standing on the new field (people whose homes were
    /// bought out, their parked cars, a camp) moves off it to the nearest
    /// edge: nothing stands about on the runways.
    fn clear_field_of_people(&mut self, aid: AirportId) {
        let a = self.airports.airports[aid].clone();
        let (w, d) = a.size.size();
        let push = |p: Vec2| -> Option<Vec2> {
            if !Self::on_field(&a, p, 6.0) {
                return None;
            }
            let l = a.local(p);
            let (hw, hd) = (w * 0.5 + 25.0, d * 0.5 + 25.0);
            let to_x = hw - l.x.abs();
            let to_y = hd - l.y.abs();
            let q = if to_y <= to_x { Vec2::new(l.x, hd * l.y.signum().max(0.0) * 2.0 - hd) } else { Vec2::new(hw * if l.x >= 0.0 { 1.0 } else { -1.0 }, l.y) };
            // Off the landside if the nearer edge is the airside one, unless
            // a side edge is nearer still.
            Some(a.world(q))
        };
        for c in self.citizens.values_mut() {
            if c.inside.is_none() {
                if let Some(q) = push(c.pos) {
                    c.pos = q;
                    c.prev_pos = q;
                }
            }
        }
        let cars: Vec<(crate::vehicle::VehicleId, Vec2)> = self.vehicles.cars.iter().filter_map(|(id, c)| c.spot().and_then(|s| push(s.pos)).map(|q| (id, q))).collect();
        for (vid, q) in cars {
            self.park_stranded(vid, q);
        }
        for camp in self.society.homeless.camps.iter_mut() {
            if let Some(q) = push(camp.pos) {
                camp.pos = q;
            }
        }
    }

    /// Is `p` on or near the field of `a` (its footprint and a margin)?
    fn on_field(a: &Airport, p: Vec2, margin: f32) -> bool {
        let (w, d) = a.size.size();
        let l = a.local(p);
        l.x.abs() <= w * 0.5 + margin && l.y.abs() <= d * 0.5 + margin
    }

    /// The streets across the site of `a`, in metres of street to clear; or
    /// why it cannot be cleared (a highway, a railway, a bridge).
    fn airport_ways_under(&self, a: &Airport) -> Result<f32, String> {
        let fp = a.footprint();
        let (lo, hi) = fp.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
        let mut metres = 0.0;
        for sid in self.roads.segments_in_box(lo - Vec2::splat(20.0), hi + Vec2::splat(20.0)) {
            let s = self.roads.segment(sid);
            if s.kind == RoadKind::Path || s.tunnel {
                continue;
            }
            let (p0, p1) = self.roads.segment_points(sid);
            let n = (p0.distance(p1) / 4.0).ceil().max(1.0) as usize;
            let inside = (0..=n).filter(|i| Self::on_field(a, p0.lerp(p1, *i as f32 / n as f32), s.half_width())).count();
            if inside == 0 {
                continue;
            }
            if s.is_rail() || s.is_motorway() || s.bridge || self.roads.spans_lots(sid) {
                let at = p0.lerp(p1, 0.5);
                let way = if s.is_rail() { "railway" } else if s.bridge { "bridge" } else { "highway" };
                return Err(format!("a {way} runs across the site at ({:.0}, {:.0}): it cannot be cleared for {}", at.x, at.y, crate::names::a(a.size.label())));
            }
            metres += p0.distance(p1) * inside as f32 / (n + 1) as f32;
        }
        Ok(metres)
    }

    /// Take the streets across the site of `a` out of the plan (the ways
    /// either side stay): cut the planned roads where they run over the
    /// field and a little round it.
    fn clear_airport_site(&mut self, a: &Airport) {
        let mut cuts: Vec<(GestureId, f32, f32)> = Vec::new();
        for (id, g) in self.effective_plan().gestures.iter() {
            let Gesture::Road { kind: RoadKind::Street, .. } = g else { continue };
            if g.is_elevated() {
                continue;
            }
            let line = g.road_polyline();
            let mut s = 0.0;
            let mut run: Option<(f32, f32)> = None;
            let mut runs = Vec::new();
            for w in line.windows(2) {
                let len = w[0].distance(w[1]);
                let n = (len / 3.0).ceil().max(1.0) as usize;
                for i in 0..n {
                    let t = i as f32 / n as f32;
                    let at = s + len * t;
                    if Self::on_field(a, w[0].lerp(w[1], t), 14.0) {
                        run = Some(run.map_or((at, at), |(r0, _)| (r0, at)));
                    } else if let Some(r) = run.take() {
                        runs.push(r);
                    }
                }
                s += len;
            }
            if let Some(r) = run.take() {
                runs.push((r.0, s));
            }
            // Later stretches first: cutting one keeps the arcs before it.
            for (r0, r1) in runs.into_iter().rev() {
                cuts.push((*id, (r0 - 3.0).max(0.0), (r1 + 3.0).min(s)));
            }
        }
        for (id, s0, s1) in cuts {
            self.remove_road_piece(id, s0, s1);
        }
    }

    /// [airport] Build an airport near `p` (see [`World::plan_airport`]):
    /// grown buildings on the site are bought out, the ground is levelled,
    /// a forecourt road is laid along its landside unless a street runs
    /// there already, and it gets a lot and a building of its own. It runs
    /// once that road reaches the city.
    pub fn place_airport(&mut self, size: AirportSize, p: Vec2, heading_deg: Option<f32>) -> Result<BuildingId, String> {
        self.place_airport_with(size, p, heading_deg, false)
    }

    /// [airport] [`World::place_airport`]; with `clear`, the streets across
    /// the site go first (`plan_airport_with`).
    pub fn place_airport_with(&mut self, size: AirportSize, p: Vec2, heading_deg: Option<f32>, clear: bool) -> Result<BuildingId, String> {
        let mut a = self.plan_airport_with(size, p, heading_deg, clear)?;
        for (_, lot) in self.buildings_under(&a.footprint()) {
            self.take_over_lot(lot, size.label())?;
        }
        if clear {
            let under = self.buildings_under(&a.footprint());
            let (tall, _) = self.approach_obstacles(&a, a.level, &under);
            for (_, lot) in tall {
                self.take_over_lot(lot, &format!("clear the approach to {}", crate::names::a(size.label())))?;
            }
        }
        self.economy.debit_item(Flow::Construction, size.cost() + a.earthwork_cost, Item::Named(size.label().to_string()));
        const NAMES: [&str; 12] = ["Kessler", "Morrow", "Saltmarsh", "Greyhaven", "Tideway", "Ironbank", "Halcyon", "Calloway", "Ashgate", "Driftmoor", "Vantage", "Cinderfield"];
        let h = ap::mix(self.config.seed ^ SALT, (a.center.x as i64 as u64) ^ ((a.center.y as i64 as u64) << 20));
        let n = NAMES[(h % NAMES.len() as u64) as usize];
        a.name = match size {
            AirportSize::Airfield => format!("{n} Airfield"),
            AirportSize::International => format!("{n} International"),
        };
        let probe = a.front_mid() + a.across() * rail::ROAD_GAP;
        let fused = self.roads.nearest_street(probe).is_some_and(|(_, _, q)| q.distance(probe) <= rail::ROAD_EDGE_FUSE);
        if !fused {
            a.stub = Some(a.forecourt());
        }
        a.dir = self.wind_landing_dir(&a);
        let stub = a.stub;
        let name = a.name.clone();
        let aid = self.airports.airports.insert(a);
        let had_project = self.project.take();
        if clear && self.airports.airports[aid].cleared_m > 0.0 {
            let plan = self.airports.airports[aid].clone();
            self.clear_airport_site(&plan);
        }
        if let Some(stub) = stub {
            self.add_gesture(Gesture::Road { points: stub.to_vec(), lanes_fwd: 2, lanes_bwd: 2, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        }
        if self.project.is_some() {
            self.implement();
        } else {
            self.rebuild_from_master();
        }
        self.project = had_project;
        let Some(lot_id) = self.airports.airports[aid].lot else {
            self.airports.airports.remove(aid);
            self.rebuild_from_master();
            return Err("the airport found no street to front".into());
        };
        let parking = if size == AirportSize::International { 900 } else { 160 };
        let id = self.place_special(lot_id, name, size.jobs(), parking, |b| {
            b.airport = Some(size);
            b.floors = 2;
        });
        // Drawn as itself from the start (what it bought out is cleared).
        self.buildings[id].demolition = None;
        self.airports.airports[aid].building = Some(id);
        self.clear_field_of_people(aid);
        self.refresh_airport_links();
        let tick = self.clock.tick;
        self.airport_timetable(aid, tick);
        self.events.push(SimEvent::LotsChanged);
        Ok(id)
    }

    /// The lot an airport stands on: its whole footprint, fronting the
    /// nearest street along its landside (normally its forecourt road).
    pub(in crate::world) fn airport_lot(&self, aid: AirportId) -> Option<Lot> {
        let a = &self.airports.airports[aid];
        let corners = a.corners();
        let probe = a.front_mid() + a.across() * rail::ROAD_GAP;
        let (front_segment, front_t, car_point) = self.roads.nearest_street(probe)?;
        if car_point.distance(probe) > 60.0 {
            return None;
        }
        Some(Lot {
            zone: ZoneId::default(),
            land_use: LandUse::Civic,
            polygon: corners.to_vec(),
            corners,
            front_edge: [corners[0], corners[1]],
            center: a.center,
            front_dir: a.across(),
            front_segment,
            front_t,
            access_node: self.roads.nearest_endpoint(front_segment, front_t),
            building: None,
            landlocked: false,
            footfall_today: 0,
            footfall_hour: 0,
            car_segment: Some(front_segment),
            car_point,
            park: None,
            shore: Default::default(),
        })
    }

    /// Recompute whether each airport's road reaches the city; drop
    /// airports whose building has gone.
    pub(in crate::world) fn refresh_airport_links(&mut self) {
        if self.airports.is_empty() {
            return;
        }
        let gone: Vec<AirportId> = self.airports.airports.iter().filter(|(_, a)| a.building.is_some_and(|b| !self.buildings.contains_key(b))).map(|(id, _)| id).collect();
        for id in gone {
            self.close_airport(id);
            self.airports.airports.remove(id);
        }
        let (_, drive) = self.network_components();
        let ids: Vec<AirportId> = self.airports.airports.keys().collect();
        for id in ids {
            let node = self.airports.airports[id].lot.and_then(|l| self.lots.get(l)).map(|l| l.access_node);
            let ok = node.and_then(|n| self.drive_component(n)).is_some_and(|c| drive.contains(&c));
            self.airports.airports[id].road_connected = ok;
        }
    }

    /// [airport] Pull down an airport: its planes divert, imports waiting
    /// for a flight come by road, the ground goes back to the plan. Its
    /// forecourt road stays a road.
    pub(in crate::world) fn bulldoze_airport(&mut self, bid: BuildingId) -> Result<String, String> {
        let aid = self.airports.by_building(bid).map(|(id, _)| id).ok_or("no such airport")?;
        let name = self.airports.airports[aid].name.clone();
        self.close_airport(aid);
        self.airports.airports.remove(aid);
        self.demolish(bid);
        let had_project = self.project.take();
        self.rebuild_from_master();
        self.project = had_project;
        self.events.push(SimEvent::LotsChanged);
        Ok(format!("{name} bulldozed with its runways"))
    }

    /// Everything an airport holds goes on by road; its planes and cars go.
    fn close_airport(&mut self, aid: AirportId) {
        let Some(a) = self.airports.airports.get_mut(aid) else { return };
        let loads: Vec<AirLoad> = a.cargo_waiting.drain(..).chain(a.cargo_landed.drain(..)).collect();
        a.planes.clear();
        for r in a.rotations.iter_mut() {
            r.cancelled = r.cancelled || !r.started;
        }
        let cars = std::mem::take(&mut a.cars);
        for c in cars {
            if self.traffic.cars.contains_key(c) {
                self.traffic.remove_car(c);
            }
        }
        for l in loads {
            if self.buildings.contains_key(l.cargo.to) {
                self.dispatch_truck(TruckOrigin::CityEdge, Cargo { from: BuildingId::default(), ..l.cargo }, 6);
            }
        }
    }

    /// Is airport `aid` running: standing, staffed enough to open, its
    /// road reaching the city?
    pub fn airport_works(&self, aid: AirportId) -> bool {
        let Some(a) = self.airports.airports.get(aid) else { return false };
        a.road_connected && a.building.and_then(|b| self.buildings.get(b)).is_some_and(|b| b.operating() && !b.is_derelict())
    }

    /// Why an airport does not run, if it does not.
    pub fn airport_problem(&self, aid: AirportId) -> Option<String> {
        let a = self.airports.airports.get(aid)?;
        if !a.road_connected {
            return Some("not connected to the city's roads yet: draw a road to its forecourt road".into());
        }
        let b = a.building.and_then(|b| self.buildings.get(b))?;
        if b.is_derelict() {
            return Some("derelict".into());
        }
        if !b.operating() {
            return Some("not open (staff or power)".into());
        }
        None
    }

    /// The working airports' buildings.
    pub fn working_airports(&self) -> Vec<BuildingId> {
        self.airports.airports.iter().filter(|(id, _)| self.airport_works(*id)).filter_map(|(_, a)| a.building).collect()
    }

    /// Which way it lands: into the wind (the way the air moves is
    /// `weather::wind_dir`); a dead calm keeps the way it had.
    fn wind_landing_dir(&self, a: &Airport) -> f32 {
        if let Some(d) = a.one_way {
            return d;
        }
        let w = crate::weather::wind_dir(self.clock.tick);
        let d = a.along.dot(w);
        if d > 0.2 {
            -1.0
        } else if d < -0.2 {
            1.0
        } else {
            a.dir
        }
    }

    /// [airport] The height limit (metres above the sea, as `lot_pad`) the
    /// approach surfaces put over `p`, if any.
    pub fn airport_height_limit(&self, p: Vec2) -> Option<f32> {
        self.airports.height_limit(p)
    }

    // ── the tick ───────────────────────────────────────────────────────────

    pub(super) fn step_airports(&mut self) {
        if self.airports.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        let ids: Vec<AirportId> = self.airports.airports.keys().collect();
        for aid in ids {
            if !self.airport_works(aid) {
                // Shut: nothing flies; planes on the field stay put.
                continue;
            }
            if tick % 60 == 7 {
                let a = &self.airports.airports[aid];
                let dir = self.wind_landing_dir(a);
                self.airports.airports[aid].dir = dir;
            }
            self.airport_arrivals(aid, tick);
            self.airport_planes(aid, tick);
            if tick % 3 == 2 {
                self.airport_unload(aid);
            }
        }
    }

    /// Make the timetable from `from_tick` to the end of the day.
    pub(super) fn airport_timetable(&mut self, aid: AirportId, from_tick: u64) {
        let pax = self.airport_demand();
        let size = self.airports.airports[aid].size;
        let cargo = self.airports.airports[aid].cargo_waiting.iter().map(|l| l.cargo.amount).sum::<f32>() + self.airports.airports[aid].cargo_out + self.airports.airports[aid].stats.yesterday.cargo_in;
        let day = self.clock.day();
        let a = &mut self.airports.airports[aid];
        let rot = ap::timetable(size, day, from_tick, pax, cargo, self.config.seed ^ SALT ^ (a.center.x.to_bits() as u64), a.next_flight);
        a.next_flight = a.next_flight.wrapping_add(rot.len() as u32 * 2 + 2) % 9000 + 100;
        // Rotations of yesterday still under way (late ones) stay.
        a.rotations.retain(|r| r.started && a.planes.iter().any(|p| p.flight == r.flight));
        a.rotations.extend(rot);
        a.stats.demand = pax;
    }

    /// Passengers a day each way the city would fly.
    fn airport_demand(&self) -> f32 {
        let offices: u32 = self.buildings.values().filter(|b| b.land_use == LandUse::Commercial && b.operating()).map(|b| b.jobs as u32).sum();
        ap::daily_pax(self.citizens.len(), self.tourism.demand_per_day, offices)
    }

    /// Ticks the approach takes from the map edge (so a flight timetabled
    /// to land at T starts its approach that much before).
    fn approach_len(&self, a: &Airport, dir: f32) -> f32 {
        let lay = a.layout();
        let r = lay.runways[lay.arrivals];
        let th = a.world(Vec2::new(-dir * r.length * 0.5, r.y));
        (edge_distance(th, -a.along * dir) + 500.0).clamp(1800.0, 4200.0)
    }

    fn climb_len(&self, a: &Airport, dir: f32) -> f32 {
        let lay = a.layout();
        let r = lay.runways[lay.departures];
        let lift = a.world(Vec2::new(-dir * r.length * 0.5 + dir * (20.0 + ap::takeoff_roll(a.size)), r.y));
        (edge_distance(lift, a.along * dir) + 600.0).clamp(1800.0, 4200.0)
    }

    /// Weather that holds flights off the map: a gale or a storm.
    fn grounded_by_weather(&self) -> bool {
        self.weather.wind > 0.92 || (self.weather.spell == crate::weather::Spell::Storm && self.weather.wind > 0.8)
    }

    /// Flights due in start their approach when the runway is clear and a
    /// stand is free; held off the map (late) otherwise, cancelled at last.
    fn airport_arrivals(&mut self, aid: AirportId, tick: u64) {
        let tsp = self.config.traffic_seconds_per_tick;
        let weather = self.grounded_by_weather();
        let n = self.airports.airports[aid].rotations.len();
        for i in 0..n {
            let a = &self.airports.airports[aid];
            let r = &a.rotations[i];
            if r.started || r.cancelled {
                continue;
            }
            let dir = a.dir;
            let approach_len = self.approach_len(a, dir);
            let climb_len = self.climb_len(a, dir);
            let mut plane = Plane { kind: r.kind, airline: r.airline, flight: r.flight, from: r.from, to: r.to, phase: PlanePhase::Approach, since: tick, until: tick, stand: 0, dir, depart: r.depart, pax_in: 0, pax_out: 0, cargo_in: 0.0, late_in: 0, approach_len, climb_len };
            let ticks = ap::phase_ticks(a.size, PlanePhase::Approach, &plane, tsp);
            let due = r.arrive.saturating_sub(ticks);
            if tick < due {
                continue;
            }
            let late = tick - due;
            if late > ap::CANCEL_AFTER {
                let a = &mut self.airports.airports[aid];
                a.rotations[i].cancelled = true;
                a.stats.today.cancelled += 1;
                continue;
            }
            if weather {
                continue;
            }
            let lay = a.layout();
            // Spacing on the arrival runway, and on a single runway the
            // departures rolling on it.
            let single = lay.arrivals == lay.departures;
            let busy = a.planes.iter().any(|p| (p.phase == PlanePhase::Approach && tick < p.since + ap::SEPARATION) || (single && matches!(p.phase, PlanePhase::Takeoff)) || (p.phase == PlanePhase::Approach && p.until.abs_diff(tick + ticks) < ap::SEPARATION));
            if busy {
                continue;
            }
            let Some(stand) = a.free_stand(r.kind) else { continue };
            plane.stand = stand as u8;
            plane.until = tick + ticks;
            plane.late_in = late as u32;
            let a = &mut self.airports.airports[aid];
            a.rotations[i].started = true;
            a.planes.push(plane);
        }
    }

    /// Every plane whose phase is over moves on to the next.
    fn airport_planes(&mut self, aid: AirportId, tick: u64) {
        let tsp = self.config.traffic_seconds_per_tick;
        let mut i = 0;
        while i < self.airports.airports[aid].planes.len() {
            let a = &self.airports.airports[aid];
            let p = a.planes[i].clone();
            let p = &p;
            if tick < p.until {
                i += 1;
                continue;
            }
            let size = a.size;
            let lay = a.layout();
            let next = match p.phase {
                PlanePhase::Approach => {
                    self.plane_landed(aid, i);
                    Some(PlanePhase::Rollout)
                }
                PlanePhase::Rollout => Some(PlanePhase::TaxiIn),
                PlanePhase::TaxiIn => {
                    self.plane_on_stand(aid, i);
                    Some(PlanePhase::AtStand)
                }
                PlanePhase::AtStand => {
                    if tick >= p.depart {
                        self.plane_boards(aid, i);
                        Some(PlanePhase::TaxiOut)
                    } else {
                        self.airports.airports[aid].planes[i].until = p.depart;
                        None
                    }
                }
                PlanePhase::TaxiOut => Some(PlanePhase::HoldShort),
                PlanePhase::HoldShort => {
                    let single = lay.arrivals == lay.departures;
                    let roll = ap::phase_ticks(size, PlanePhase::Takeoff, p, tsp);
                    let clear = !a.planes.iter().enumerate().any(|(j, o)| j != i && (o.phase == PlanePhase::Takeoff || (single && (o.phase == PlanePhase::Rollout || (o.phase == PlanePhase::Approach && o.until <= tick + roll + 1)))));
                    if clear && !self.grounded_by_weather() {
                        Some(PlanePhase::Takeoff)
                    } else {
                        None
                    }
                }
                PlanePhase::Takeoff => {
                    self.plane_took_off(aid, i);
                    Some(PlanePhase::Climb)
                }
                PlanePhase::Climb => {
                    let a = &mut self.airports.airports[aid];
                    let flight = a.planes[i].flight;
                    a.planes.remove(i);
                    a.rotations.retain(|r| r.flight != flight || !r.started);
                    continue;
                }
            };
            if let Some(ph) = next {
                let a = &mut self.airports.airports[aid];
                let p = &mut a.planes[i];
                p.phase = ph;
                p.since = tick;
                p.until = match ph {
                    PlanePhase::AtStand => tick + p.kind.turnaround().min(p.depart.saturating_sub(tick).max(10)),
                    PlanePhase::HoldShort => tick + 1,
                    _ => tick + ap::phase_ticks(size, ph, p, tsp),
                };
                if ph == PlanePhase::AtStand {
                    p.until = p.until.max(tick + p.kind.turnaround() / 2);
                }
            }
            i += 1;
        }
    }

    /// Touchdown: the landing charge, noise along the approach.
    fn plane_landed(&mut self, aid: AirportId, i: usize) {
        let a = &self.airports.airports[aid];
        let p = a.planes[i].clone();
        let fee = p.kind.landing_fee();
        let bid = a.building;
        self.airport_noise(aid, &p, true);
        let a = &mut self.airports.airports[aid];
        a.stats.today.arrivals += 1;
        a.stats.movements_total += 1;
        a.stats.today.fees += fee;
        if p.late_in as u64 >= ap::DELAYED_AFTER {
            a.stats.today.delayed += 1;
        }
        a.stats.today.delay_minutes += p.late_in;
        if let Some(b) = bid {
            self.economy.credit_item(Flow::AirportFees, fee, Item::Building(b));
        }
    }

    /// On the stand: passengers off and into town, cargo off for the lorries.
    fn plane_on_stand(&mut self, aid: AirportId, i: usize) {
        let tick = self.clock.tick;
        let a = &self.airports.airports[aid];
        let p = a.planes[i].clone();
        let load = a.rotations.iter().find(|r| r.flight == p.flight).map_or(0.75, |r| r.load);
        let pax = (p.kind.seats() as f32 * load).round() as u32;
        // Who they are: the shares of the demand.
        let (residents, tourists, business) = self.airport_shares();
        let tour = (pax as f32 * tourists).round() as u32;
        let biz = (pax as f32 * business).round() as u32;
        let _ = residents;
        let bid = a.building;
        // Cargo off: freighters take everything waiting, the rest their belly.
        let mut room = p.kind.cargo_units();
        let a = &mut self.airports.airports[aid];
        let mut landed = 0.0;
        let mut k = 0;
        while k < a.cargo_waiting.len() {
            let amt = a.cargo_waiting[k].cargo.amount;
            if amt <= room || landed == 0.0 && p.kind == PlaneKind::Freighter {
                room -= amt;
                landed += amt;
                let l = a.cargo_waiting.remove(k);
                a.cargo_landed.push(AirLoad { since: tick, ..l });
            } else {
                k += 1;
            }
        }
        a.planes[i].pax_in = pax;
        a.planes[i].cargo_in = landed;
        a.stats.today.pax_in += pax;
        a.stats.pax_total += pax as u64;
        a.stats.today.business += biz;
        a.stats.today.cargo_in += landed;
        // Visitors check into the hotels.
        let checked = match bid {
            Some(b) if tour > 0 => self.air_tourists(b, tour as usize, ap::mix(p.flight as u64, tick)),
            _ => 0,
        };
        self.airports.airports[aid].stats.today.tourists_in += checked;
        self.airport_ground_trips(aid, pax, true, ap::mix(p.flight as u64 ^ 0x55, tick));
    }

    /// Doors shut: the passengers out board (the fee per head), exports
    /// go into the hold.
    fn plane_boards(&mut self, aid: AirportId, i: usize) {
        let tick = self.clock.tick;
        let a = &self.airports.airports[aid];
        let p = a.planes[i].clone();
        let load = a.rotations.iter().find(|r| r.flight == p.flight).map_or(0.75, |r| r.load);
        let pax = (p.kind.seats() as f32 * load).round() as u32;
        let (residents, _, _) = self.airport_shares();
        let bid = a.building;
        let fee = pax as f32 * ap::PAX_FEE;
        // Out the way it takes off now: into the wind, or away from a
        // closed end.
        let dep = match a.one_way {
            Some(d) => -d,
            None => a.dir,
        };
        let climb = self.climb_len(a, dep);
        let a = &mut self.airports.airports[aid];
        a.planes[i].dir = dep;
        a.planes[i].climb_len = climb;
        let out = a.cargo_out.min(p.kind.cargo_units());
        a.cargo_out -= out;
        a.planes[i].pax_out = pax;
        a.stats.today.pax_out += pax;
        a.stats.pax_total += pax as u64;
        a.stats.today.residents_out += (pax as f32 * residents).round() as u32;
        a.stats.today.cargo_out += out;
        a.stats.today.fees += fee;
        if let Some(b) = bid {
            self.economy.credit_item(Flow::AirportFees, fee, Item::Building(b));
        }
        let _ = tick;
    }

    /// Wheels up: lateness out, noise along the climb.
    fn plane_took_off(&mut self, aid: AirportId, i: usize) {
        let tick = self.clock.tick;
        let p = self.airports.airports[aid].planes[i].clone();
        self.airport_noise(aid, &p, false);
        let late = tick.saturating_sub(p.depart);
        let a = &mut self.airports.airports[aid];
        a.stats.today.departures += 1;
        a.stats.movements_total += 1;
        if late >= ap::DELAYED_AFTER {
            a.stats.today.delayed += 1;
        }
        a.stats.today.delay_minutes += late as u32;
    }

    /// Residents, visitors and business travellers as shares of the
    /// passengers (by the demand that makes the timetable).
    fn airport_shares(&self) -> (f32, f32, f32) {
        let offices: u32 = 0;
        let _ = offices;
        let r = self.citizens.len() as f32 * ap::RESIDENT_TRIPS;
        let t = self.tourism.demand_per_day.max(0.0) * ap::TOURIST_AIR_SHARE;
        let total = self.airports.airports.values().map(|a| a.stats.demand).fold(0.0, f32::max).max(r + t).max(1.0);
        let b = (total - r - t).max(0.0);
        (r / total, t / total, b / total)
    }

    /// Log one movement's noise on the air noise grid along its path: the
    /// approach in, or the climb out, loudest at the runway end.
    fn airport_noise(&mut self, aid: AirportId, p: &Plane, arriving: bool) {
        let a = &self.airports.airports[aid];
        let lay = a.layout();
        let r = if arriving { lay.runways[lay.arrivals] } else { lay.runways[lay.departures] };
        let (start, out) = if arriving {
            (a.world(Vec2::new(-p.dir * r.length * 0.5, r.y)), -a.along * p.dir)
        } else {
            (a.world(Vec2::new(-p.dir * r.length * 0.5 + p.dir * (20.0 + ap::takeoff_roll(a.size)), r.y)), a.along * p.dir)
        };
        let across = a.across();
        let loud = ap::MOVEMENT_NOISE * p.kind.noise() * if arriving { 0.8 } else { 1.0 };
        let step = crate::air::NOISE_CELL * 0.5;
        let mut d = -r.length * 0.3;
        while d <= ap::NOISE_REACH {
            let k = (1.0 - d.max(0.0) / ap::NOISE_REACH).powf(1.4);
            let q = start + out * d;
            for (off, w) in [(0.0f32, 1.0f32), (-1.0, 0.45), (1.0, 0.45)] {
                let c = crate::air::noise_cell(q + across * off * crate::air::NOISE_CELL);
                *self.air.noise.entry(c).or_insert(0.0) += loud * k * w * 0.5;
            }
            d += step;
        }
    }

    // ── to and from town ───────────────────────────────────────────────────

    /// Transit that serves the terminal: a railway or Central Station, a
    /// metro station, a bus stop in walking reach of its door.
    pub fn airport_transit(&self, aid: AirportId) -> (Option<BuildingId>, Option<BuildingId>, bool) {
        let Some(a) = self.airports.airports.get(aid) else { return (None, None, false) };
        let door = a.front_mid();
        let near = |b: &BuildingId| self.building_entrance(*b).is_some_and(|q| q.distance(door) <= ap::TRANSIT_REACH);
        let rail = self.stations.iter().copied().find(near);
        let metro = self.buildings.iter().filter(|(_, b)| b.metro && b.operating()).map(|(id, _)| id).find(near);
        let bus = self.transit.routes.values().any(|r| r.stops.iter().any(|s| s.pos.distance(door) <= ap::TRANSIT_REACH));
        (rail, metro, bus)
    }

    /// How a planeload gets to (or from) town, by `Access` share.
    pub(super) fn access_split(&self, aid: AirportId) -> [f32; 6] {
        let (rail, metro, bus) = self.airport_transit(aid);
        let mut s = [0.0f32; 6];
        let mut left = 1.0;
        if rail.is_some() {
            s[3] = 0.3;
            left -= 0.3;
        }
        if metro.is_some() {
            s[4] = 0.22;
            left -= 0.22;
        }
        if bus {
            s[5] = 0.1;
            left -= 0.1;
        }
        // Without a train the airport runs its own coaches.
        let coach = if rail.is_none() && metro.is_none() { 0.15 } else { 0.0 };
        s[2] = coach;
        left -= coach;
        s[1] = left * 0.4;
        s[0] = left * 0.6;
        s
    }

    /// A planeload's trips into town (or out to the airport): counted by
    /// way, the bus stops by the terminal credited with theirs, and a few
    /// real cars, cabs and a coach put on the road between the terminal and
    /// homes, hotels and offices.
    fn airport_ground_trips(&mut self, aid: AirportId, pax: u32, inbound: bool, seed: u64) {
        let split = self.access_split(aid);
        let mut counts = [0u32; 6];
        for (k, s) in split.iter().enumerate() {
            counts[k] = (pax as f32 * s).round() as u32;
        }
        {
            let a = &mut self.airports.airports[aid];
            for k in 0..6 {
                a.stats.today.access[k] += counts[k];
            }
        }
        // The stops by the door count their riders.
        if counts[5] > 0 {
            if let Some(door) = self.airports.airports.get(aid).map(|a| a.front_mid()) {
                let stop = self.transit.routes.values_mut().flat_map(|r| r.stops.iter_mut()).filter(|s| s.pos.distance(door) <= ap::TRANSIT_REACH).min_by(|a, b| a.pos.distance(door).total_cmp(&b.pos.distance(door)));
                if let Some(s) = stop {
                    if inbound {
                        s.stats.on_today += counts[5];
                    } else {
                        s.stats.off_today += counts[5];
                    }
                }
            }
        }
        // Real traffic: a car per 30 drivers, a cab per 20 riders, a coach.
        let cars = (counts[0] / 30).clamp(if counts[0] > 0 { 1 } else { 0 }, 5);
        let cabs = (counts[1] / 20).clamp(if counts[1] > 0 { 1 } else { 0 }, 4);
        let coach = counts[2] > 0;
        let mut h = seed;
        for k in 0..(cars + cabs + coach as u32) {
            h = ap::mix(h, k as u64 + 17);
            let kind = if k < cars { 0 } else if k < cars + cabs { 1 } else { 2 };
            self.airport_car(aid, inbound, kind, h);
        }
    }

    /// One car between the terminal kerb and a home, hotel or office.
    fn airport_car(&mut self, aid: AirportId, inbound: bool, kind: u8, h: u64) {
        let Some(a) = self.airports.airports.get(aid) else { return };
        if a.cars.len() >= ap::MAX_CARS {
            return;
        }
        let Some(apt) = a.building else { return };
        // Somewhere in town, by a hash over the buildings with people in
        // (the list made again each day).
        if self.airports.targets.is_empty() || self.airports.targets_day != self.clock.day() {
            self.airports.targets = self.buildings.iter().filter(|(_, b)| !b.is_special() && (b.homes > 0 || b.jobs > 0)).map(|(id, _)| id).collect();
            self.airports.targets_day = self.clock.day();
        }
        let n = self.airports.targets.len();
        if n == 0 {
            return;
        }
        let pick = (0..6u64).find_map(|t| {
            let id = self.airports.targets[(ap::mix(h, t) % n as u64) as usize];
            self.buildings.get(id).is_some_and(|b| id != apt && b.operating()).then_some(id)
        });
        let Some(other) = pick else { return };
        let Some((sa, pa)) = self.truck_stop(apt) else { return };
        let Some((sb, pb)) = self.truck_stop(other) else { return };
        let route = if inbound { self.traffic.net.find_route(sa, pa, sb, pb) } else { self.traffic.net.find_route(sb, pb, sa, pa) };
        let Some((route, s0, s1)) = route else { return };
        let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, (h % 8) as u8);
        let mut rng = crate::vehicle::model_rng(h, slotmap::Key::data(&car).as_ffi());
        self.traffic.cars[car].model = match kind {
            0 => crate::vehicle::VehicleModel::for_owner(30.0 + (h % 90) as f32, 25 + (h % 50) as u8, 0.5, &mut rng),
            1 => crate::taxi::cab_model((h % 40) as u16),
            _ => crate::vehicle::VehicleModel::bus(5, &mut rng),
        };
        self.airports.airports[aid].cars.push(car);
    }

    /// An airport's car at the end of its run leaves the road. False when
    /// `car` is not one of theirs.
    pub(super) fn airport_car_arrived(&mut self, car: CarId) -> bool {
        if self.airports.is_empty() {
            return false;
        }
        for a in self.airports.airports.values_mut() {
            if let Some(k) = a.cars.iter().position(|c| *c == car) {
                a.cars.swap_remove(k);
                self.traffic.remove_car(car);
                return true;
            }
        }
        false
    }

    /// [airport] Visitors off a plane at `port`: `n` of them look for a bed
    /// (their own kind of hotel first, as those who come by road), check
    /// in and are on their way there. How many found one.
    fn air_tourists(&mut self, port: BuildingId, n: usize, seed: u64) -> u32 {
        let tick = self.clock.tick;
        let mut free: Vec<(BuildingId, i32, Tier)> = self.hotels().into_iter().filter(|(_, beds, _)| *beds > 0).map(|(id, beds, guests)| (id, beds as i32 - guests as i32, self.buildings[id].tier)).filter(|f| f.1 > 0).collect();
        if free.is_empty() {
            self.tourism.stats.turned_away_today += n as u32;
            return 0;
        }
        let start = self.building_entrance(port).unwrap_or_default();
        let mut placed = 0;
        for k in 0..n {
            if self.tourism.tourists.len() >= tm::MAX_TOURISTS {
                self.tourism.stats.turned_away_today += (n - k) as u32;
                break;
            }
            let s = ap::mix(seed, k as u64 + 1);
            let mut rng = crate::rng_for(s, SALT);
            let class = match rng.random::<f32>() {
                x if x < 0.3 => Tier::Low,
                x if x < 0.8 => Tier::Mid,
                _ => Tier::High,
            };
            let pick = free.iter().position(|f| f.1 > 0 && f.2 == class).or_else(|| free.iter().position(|f| f.1 > 0));
            let Some(pick) = pick else {
                self.tourism.stats.turned_away_today += (n - k) as u32;
                break;
            };
            free[pick].1 -= 1;
            let hotel = free[pick].0;
            let nights = 1 + rng.random_range(0..=tm::MAX_EXTRA_NIGHTS);
            let budget = Tourist::daily_budget(class) * (nights + 1) as f32 * rng.random_range(0.8..1.25);
            let rooms = self.night_price_at(hotel) * (1.0 + self.economy.taxes.tourist_tax()) * nights as f32;
            let tourist = Tourist {
                seed: s,
                age: rng.random_range(19..72u8),
                class,
                pos: start,
                prev_pos: start,
                heading: 0.0,
                prev_heading: 0.0,
                hotel,
                via: Arrival::Air,
                port: Some(port),
                arrived_tick: tick,
                leave_day: self.clock.day() + nights as u64,
                money: budget + rooms,
                spent: 0.0,
                state: TouristState::InHotel,
                next_tick: tick + 1,
                sights: 0,
                commuter: None,
            };
            let id = self.tourism.tourists.insert(tourist);
            let st = &mut self.tourism.stats;
            st.arrivals_today += 1;
            st.arrivals_total += 1;
            st.arrivals_by_mode_today[Arrival::Air.index()] += 1;
            let to = self.building_entrance(hotel).unwrap_or(start);
            self.tourist_leg(id, Goal::Hotel, to);
            placed += 1;
        }
        placed
    }

    // ── air cargo ──────────────────────────────────────────────────────────

    /// [airport] An import that comes by air: electronics and art (and a
    /// share of consumer goods at the international airport) wait at the
    /// cargo terminal for the next flight in, then go on by lorry. False
    /// when it does not fly (no working airport, not that kind of goods).
    pub(super) fn import_by_air(&mut self, cargo: Cargo) -> bool {
        if self.airports.is_empty() {
            return false;
        }
        let tick = self.clock.tick;
        let goods_share = |a: &Airport| a.size == AirportSize::International && cargo.resource == Resource::Goods && (ap::mix(tick ^ SALT, slotmap::Key::data(&cargo.to).as_ffi()) % 1000) as f32 / 1000.0 < ap::GOODS_BY_AIR;
        let pick = self.airports.airports.iter().filter(|(id, a)| self.airport_works(*id) && a.cargo_waiting.len() < 400 && (ap::flies(cargo.resource) || goods_share(a))).max_by_key(|(_, a)| a.size).map(|(id, _)| id);
        let Some(aid) = pick else { return false };
        self.airports.airports[aid].cargo_waiting.push(AirLoad { cargo, since: tick });
        true
    }

    /// Landed loads on lorries from the cargo terminal to their buyers; the
    /// handling fee to the city. A load waiting for a flight too long comes
    /// by road after all.
    fn airport_unload(&mut self, aid: AirportId) {
        let tick = self.clock.tick;
        let Some(bid) = self.airports.airports[aid].building else { return };
        let stale: Vec<AirLoad> = {
            let a = &mut self.airports.airports[aid];
            let (old, keep): (Vec<AirLoad>, Vec<AirLoad>) = a.cargo_waiting.drain(..).partition(|l| tick.saturating_sub(l.since) > 900);
            a.cargo_waiting = keep;
            old
        };
        for l in stale {
            self.dispatch_truck(TruckOrigin::CityEdge, l.cargo, 6);
        }
        let n = self.airports.airports[aid].cargo_landed.len().min(3);
        let loads: Vec<AirLoad> = self.airports.airports[aid].cargo_landed.drain(..n).collect();
        for l in loads {
            if !self.buildings.contains_key(l.cargo.to) {
                continue;
            }
            let fee = l.cargo.price * ap::CARGO_FEE;
            self.economy.credit_item(Flow::AirportFees, fee, Item::Building(bid));
            self.airports.airports[aid].stats.today.fees += fee;
            self.queue_truck(TruckOrigin::Building(bid), Cargo { from: bid, ..l.cargo }, 6);
        }
    }

    /// [airport] Exports of `r` earn more flown out of a working airport.
    pub(super) fn air_export_bonus(&self, r: Resource) -> f32 {
        if ap::flies(r) && self.airports.airports.keys().any(|id| self.airport_works(id)) { ap::AIR_EXPORT_BONUS } else { 1.0 }
    }

    /// Dear exports going out by air: a lorry from each seller to the cargo
    /// terminal (once a few units have gathered), into the next holds out.
    pub(super) fn air_exports(&mut self, sends: Vec<(BuildingId, Resource, f32)>) {
        let Some(apt) = self.airports.airports.iter().filter(|(id, _)| self.airport_works(*id)).max_by_key(|(_, a)| a.size).and_then(|(_, a)| a.building) else { return };
        for (seller, r, units) in sends {
            if units < 1.0 {
                continue;
            }
            self.queue_truck(TruckOrigin::Building(seller), Cargo { from: seller, to: apt, resource: r, amount: units, price: 0.0 }, 6);
        }
    }

    /// Goods arriving at an airport's cargo terminal (`World::deliver`):
    /// exports for the holds. False when `cargo` is not for an airport.
    pub(super) fn airport_receive(&mut self, cargo: &Cargo) -> bool {
        if self.airports.is_empty() {
            return false;
        }
        let Some(a) = self.airports.airports.values_mut().find(|a| a.building == Some(cargo.to)) else { return false };
        a.cargo_out += cargo.amount;
        true
    }

    // ── land value, noise, the day ─────────────────────────────────────────

    /// [airport] Land value: the flight paths off both ends of each runway
    /// and the field itself, by how busy it is.
    pub(super) fn splat_airport_noise(&self, b: &mut LandValueBuilder) {
        for (id, a) in self.airports.airports.iter() {
            if !self.airport_works(id) {
                continue;
            }
            let m = a.stats.yesterday.movements().max(a.stats.today.movements()) as f32;
            let k = (0.3 + m / ap::FULL_NOISE).min(1.0);
            b.splat_nuisance(a.center, a.size.size().0 * 0.6, ap::NOISE_LAND_HIT * 0.5 * k);
            let lay = a.layout();
            for r in &lay.runways {
                for s in [-1.0f32, 1.0] {
                    let end = a.world(Vec2::new(s * r.length * 0.5, r.y));
                    let mut d = 0.0;
                    while d <= 2400.0 {
                        let w = ap::NOISE_LAND_HIT * k * (1.0 - d / 2600.0) / lay.runways.len() as f32;
                        b.splat_nuisance(end + a.along * s * d, 170.0 + d * 0.06, w);
                        d += 150.0;
                    }
                }
            }
        }
    }

    /// [airport] Midnight: the books, upkeep, the complaints from under the
    /// flight paths, tomorrow's timetable.
    pub(super) fn daily_airports(&mut self) {
        if self.airports.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        let ids: Vec<AirportId> = self.airports.airports.keys().collect();
        let (complaints, homes) = self.airport_complaints();
        for aid in ids {
            let works = self.airport_works(aid);
            let a = &mut self.airports.airports[aid];
            a.stats.today.complaints = complaints.get(&aid).copied().unwrap_or(0);
            a.stats.homes_in_noise = homes.get(&aid).copied().unwrap_or(0);
            a.stats.yesterday = std::mem::take(&mut a.stats.today);
            let (size, bid) = (a.size, a.building);
            if let Some(b) = bid {
                self.economy.debit_item(Flow::AirportUpkeep, size.upkeep_per_day(), Item::Building(b));
            }
            if works {
                self.airport_timetable(aid, tick);
            } else {
                let a = &mut self.airports.airports[aid];
                a.rotations.retain(|r| r.started);
            }
        }
    }

    /// Homes loud enough under the flight paths, per airport, and the
    /// complaints they make (a few people say so in their memories).
    fn airport_complaints(&mut self) -> (HashMap<AirportId, u32>, HashMap<AirportId, u32>) {
        let mut complaints: HashMap<AirportId, u32> = HashMap::new();
        let mut homes: HashMap<AirportId, u32> = HashMap::new();
        let near = |p: Vec2| self.airports.airports.iter().filter(|(_, a)| a.center.distance(p) < a.size.size().0 * 0.5 + ap::NOISE_REACH).min_by(|x, y| x.1.center.distance(p).total_cmp(&y.1.center.distance(p))).map(|(id, _)| id);
        let mut loud: Vec<(BuildingId, AirportId, f32)> = Vec::new();
        for (id, b) in self.buildings.iter() {
            if b.homes == 0 {
                continue;
            }
            let Some(l) = self.lots.get(b.lot) else { continue };
            let n = self.air.noise_at(l.center);
            if n < ap::COMPLAIN_NOISE {
                continue;
            }
            let Some(aid) = near(l.center) else { continue };
            *homes.entry(aid).or_default() += 1;
            loud.push((id, aid, (n / ap::COMPLAIN_NOISE).min(3.0)));
        }
        if loud.is_empty() {
            return (complaints, homes);
        }
        let set: HashMap<BuildingId, (AirportId, f32)> = loud.iter().map(|(b, a, k)| (*b, (*a, *k))).collect();
        let tick = self.clock.tick;
        let day = self.clock.day();
        const LINES: [&str; 4] = ["Jets over the roof every few minutes. You can't hear yourself think.", "Another plane rattled the windows at dawn. The airport doesn't care.", "We can't open a window for the kerosene and the roar.", "The flight path goes right over the kids' bedroom."];
        let mut said = 0;
        let ids: Vec<CitizenId> = self.citizens.keys().collect();
        for c in ids {
            let Some(home) = self.citizens[c].home else { continue };
            let Some(&(aid, k)) = set.get(&home) else { continue };
            let h = ap::mix(day ^ SALT, slotmap::Key::data(&c).as_ffi());
            if (h % 1000) as f32 >= 60.0 * k {
                continue;
            }
            *complaints.entry(aid).or_default() += 1;
            if said < 24 {
                said += 1;
                let line = LINES[(h >> 20) as usize % LINES.len()];
                let x = &mut self.citizens[c];
                x.remember(tick, line.to_string());
                x.thought = "Bloody planes.".to_string();
                x.mood = (x.mood - 0.02).max(0.0);
                self.concerns.heard(c, x, crate::concerns::Concern::Noise, 0.3);
            }
        }
        (complaints, homes)
    }

    // ── the report ────────────────────────────────────────────────────────

    /// [airport] What the inspector card and the remote API show.
    pub fn airport_report(&self, bid: BuildingId) -> Option<AirportReport> {
        let (aid, a) = self.airports.by_building(bid)?;
        let lay = a.layout();
        let b = self.buildings.get(bid)?;
        let (rail, metro, bus) = self.airport_transit(aid);
        let mut transit = Vec::new();
        if let Some(r) = rail.and_then(|r| self.buildings.get(r)) {
            transit.push(format!("rail: {}", r.name));
        }
        if let Some(m) = metro.and_then(|m| self.buildings.get(m)) {
            transit.push(format!("metro: {}", m.name));
        }
        if bus {
            transit.push("bus stop at the terminal".into());
        }
        let clock = |t: u64| format!("{:02}:{:02}", (t % 1440) / 60, t % 60);
        let tick = self.clock.tick;
        let next: Vec<String> = a.rotations.iter().filter(|r| !r.started && !r.cancelled).take(6).map(|r| format!("{} {} from {}{}", clock(r.arrive), r.call_sign(), ap::CITIES[r.from as usize % ap::CITIES.len()], if tick > r.arrive { " (late)" } else { "" })).collect();
        let heading = a.along.y.atan2(a.along.x).to_degrees().rem_euclid(180.0);
        let land_dir = (a.along * a.dir).y.atan2((a.along * a.dir).x).to_degrees().rem_euclid(360.0);
        let today = &a.stats.today;
        Some(AirportReport {
            name: a.name.clone(),
            size: a.size.label(),
            works: self.airport_works(aid),
            problem: self.airport_problem(aid),
            runways: lay.runways.len(),
            stands: lay.stands.len(),
            heading_deg: heading,
            landing_towards_deg: land_dir,
            one_way: a.one_way.is_some(),
            flights_today: today.movements(),
            flights_yesterday: a.stats.yesterday.movements(),
            scheduled_today: a.rotations.iter().filter(|r| !r.cancelled).count() * 2,
            pax_today: today.pax_in + today.pax_out,
            pax_yesterday: a.stats.yesterday.pax_in + a.stats.yesterday.pax_out,
            tourists_in_today: today.tourists_in,
            business_today: today.business,
            residents_out_today: today.residents_out,
            cargo_in_today: today.cargo_in,
            cargo_out_today: today.cargo_out,
            cargo_waiting: a.cargo_waiting.iter().map(|l| l.cargo.amount).sum(),
            delayed_today: today.delayed,
            avg_delay_min: today.avg_delay(),
            cancelled_today: today.cancelled,
            complaints_yesterday: a.stats.yesterday.complaints,
            homes_in_noise: a.stats.homes_in_noise,
            fees_today: today.fees,
            fees_yesterday: a.stats.yesterday.fees,
            upkeep_per_day: a.size.upkeep_per_day(),
            jobs: b.jobs,
            workers: b.workers.len().min(u16::MAX as usize) as u16,
            demand_pax_per_day: a.stats.demand,
            access: ap::Access::ALL.iter().map(|k| (k.label(), today.access[*k as usize] + a.stats.yesterday.access[*k as usize])).collect(),
            transit,
            planes: a.planes.iter().map(|p| PlaneRow { flight: p.call_sign(), kind: p.kind.label(), phase: p.phase.label(), from: p.from_city(), to: p.to_city(), stand: p.stand, late_min: p.late_in }).collect(),
            next_arrivals: next,
        })
    }
}

/// Metres from `p` along `dir` to the map edge.
fn edge_distance(p: Vec2, dir: Vec2) -> f32 {
    let e = crate::landscape::MAP_EDGE;
    let mut t = f32::MAX;
    for (pc, dc) in [(p.x, dir.x), (p.y, dir.y)] {
        if dc > 1e-4 {
            t = t.min((e - pc) / dc);
        } else if dc < -1e-4 {
            t = t.min((-e - pc) / dc);
        }
    }
    if t == f32::MAX { 0.0 } else { t.max(0.0) }
}

/// "east", "north-west", ... for the runway end `dir` points out of.
fn end_name(dir: Vec2) -> &'static str {
    // The sim's +y is drawn as south on the map (z down the screen).
    const NAMES: [&str; 8] = ["east", "south-east", "south", "south-west", "west", "north-west", "north", "north-east"];
    let a = dir.y.atan2(dir.x).to_degrees().rem_euclid(360.0);
    NAMES[(((a + 22.5) / 45.0) as usize) % 8]
}
