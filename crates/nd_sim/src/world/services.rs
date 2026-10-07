//! Services and emergencies: placing service buildings and attractions,
//! road-distance coverage, the hourly roll for fires, crimes and illness,
//! the 911 call, the dispatcher, the units driving out and back, fire
//! damage and spread, hospital stays, stadium nights, upkeep and the
//! schooling wage bonus. The data types live in `crate::services`.

use super::*;
use crate::services::*;
use std::cmp::Ordering;
use rand::seq::SliceRandom;
use std::collections::BinaryHeap;

/// A node and the road budget left on reaching it, ordered by budget.
#[derive(PartialEq)]
struct Budget(f32, u32);

impl Eq for Budget {}

impl PartialOrd for Budget {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Budget {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0).then(self.1.cmp(&other.1))
    }
}

/// Floors a service building is drawn and powered with, by tier.
fn service_floors(kind: ServiceKind, tier: Tier) -> u8 {
    let f: [u8; 3] = match kind {
        ServiceKind::Police => [1, 2, 4],
        ServiceKind::Fire => [1, 2, 3],
        ServiceKind::Health => [2, 4, 8],
        ServiceKind::School => [1, 2, 4],
        ServiceKind::Dispatch => [1, 2, 5],
        ServiceKind::Monument => [1, 1, 1],
        ServiceKind::Stadium => [1, 2, 3],
        ServiceKind::Jail => [1, 2, 3],
        ServiceKind::TrashDepot => [1, 1, 2],
        ServiceKind::Vertiport => [1, 2, 4],
        ServiceKind::ParkingEnforcement => [1, 1, 2],
    };
    f[tier.index()]
}

impl World {
    /// The services' own random stream, so fires, crimes and illness never
    /// shift what the other systems draw. Not saved: a loaded world reseeds
    /// it from the seed and the clock.
    pub(super) fn srng(&mut self) -> &mut crate::SimRng {
        let (seed, tick) = (self.config.seed, self.clock.tick);
        self.services.rng.get_or_insert_with(|| crate::rng_for(seed ^ 0x5E7_1CE5, tick))
    }

    // ── placing ────────────────────────────────────────────────────────────

    /// Put down anything the Buildings tool offers at `p`: a rail piece
    /// where clicked, everything else on the lot under `p` or, on unzoned
    /// ground, on a lot made for the parcels there ([free-placement]).
    pub fn place_building(&mut self, what: Placeable, p: Vec2) -> Result<BuildingId, String> {
        if let Placeable::Rail(f) = what {
            return self.place_rail_facility(f, p);
        }
        if let Placeable::Dock(k) = what {
            return self.place_port(k, p);
        }
        if let Placeable::Toll(_) = what {
            return Err("a toll booth goes on a road, not a lot (World::place_toll)".into());
        }
        self.place_on_site(what, p)
    }

    /// Put down `what` on a lot (rail pieces at the lot's centre).
    pub fn place_on_lot(&mut self, what: Placeable, lot: LotId) -> Result<BuildingId, String> {
        match what {
            Placeable::Plant(k) => self.build_power(lot, k),
            Placeable::Battery(k) => self.build_battery(lot, k),
            Placeable::BusDepot => self.build_depot(lot),
            Placeable::Service(k, t) => self.build_service(lot, k, t),
            Placeable::Rail(f) => {
                let c = self.lots.get(lot).ok_or("no such lot")?.center;
                self.place_rail_facility(f, c)
            }
            Placeable::Dock(k) => {
                let c = self.lots.get(lot).ok_or("no such lot")?.center;
                self.place_port(k, c)
            }
            Placeable::ParkingLot(size) => self.build_city_parking(lot, size),
            Placeable::MetroStation => self.build_metro_station(lot),
            Placeable::Toll(_) => Err("a toll booth goes on a road, not a lot (World::place_toll)".into()),
            Placeable::CargoHub => self.build_cargo_hub(lot),
        }
    }

    /// Why `what` cannot go on `lot`, for the placement preview; None if it can.
    pub fn placement_problem(&self, what: Placeable, lot_id: LotId) -> Option<String> {
        let Some(lot) = self.lots.get(lot_id) else { return Some("no such lot".into()) };
        self.lot_problem(what, lot, Some(lot_id))
    }

    /// [free-placement] [`World::placement_problem`] for a lot that may not
    /// exist yet (`lot_id` None: one about to be made on unzoned ground).
    pub(super) fn lot_problem(&self, what: Placeable, lot: &Lot, lot_id: Option<LotId>) -> Option<String> {
        if matches!(what, Placeable::Rail(_) | Placeable::Dock(_)) {
            return None;
        }
        if matches!(what, Placeable::Toll(_)) {
            return Some("a toll booth goes on a road".into());
        }
        // Whatever grew there is bought out; another civic structure is not.
        if let Some(b) = lot.building.and_then(|b| self.buildings.get(b)) {
            if Self::is_civic_structure(b) {
                return Some(format!("{} stands there; bulldoze it first", b.name));
            }
        }
        if lot.landlocked {
            return Some("that land has no road access".into());
        }
        if self.under_facility(lot.center) {
            return Some("that ground belongs to a station or yard".into());
        }
        // [free-placement] Zoning is no bar: only a park is kept.
        if !what.allowed_on(lot.land_use) {
            return Some(format!("that is park land: unzone it before putting a {} there", what.label()));
        }
        // [plant-groups] A small lot may join the solar or wind farm next door.
        let joins = matches!(what, Placeable::Plant(k) if lot_id.is_some_and(|id| self.small_plant_lot_joins(k, id).is_some()));
        if lot.area() < what.min_area() && !joins {
            return Some(format!("too small: a {} needs {:.0} m² (this lot is {:.0})", what.label(), what.min_area(), lot.area()));
        }
        if let Placeable::Plant(k) = what {
            if let Err(e) = self.power_site_on(lot, k) {
                return Some(e);
            }
        }
        let needs_street = matches!(what, Placeable::BusDepot | Placeable::ParkingLot(_) | Placeable::CargoHub) || matches!(what, Placeable::Service(k, _) if k.unit_kind().is_some() || k == ServiceKind::Stadium || k == ServiceKind::TrashDepot || k == ServiceKind::ParkingEnforcement);
        if needs_street && !lot.car_access() {
            return Some(format!("a {} needs a street out front", what.label()));
        }
        // [clarity] Solar and wind cost what the lot they fill makes them.
        let cost = match (what, lot_id) {
            (Placeable::Plant(k), Some(id)) => self.plant_cost_on(k, id),
            (Placeable::Plant(k), None) => k.build_cost() * k.size_scale(lot.area()),
            _ => what.cost(),
        };
        if self.economy.treasury < cost {
            return Some(format!("the treasury has {:.0}; a {} costs {:.0}", self.economy.treasury, what.label(), cost));
        }
        None
    }

    /// Build a service or attraction on a vacant lot, paid from the treasury.
    pub fn build_service(&mut self, lot_id: LotId, kind: ServiceKind, tier: Tier) -> Result<BuildingId, String> {
        let what = Placeable::Service(kind, tier);
        if let Some(problem) = self.placement_problem(what, lot_id) {
            return Err(problem);
        }
        self.take_over_lot(lot_id, kind.tier_label(tier))?;
        self.economy.debit_item(Flow::Construction, what.cost(), Item::Named(kind.tier_label(tier).to_string()));
        let n = self.srng().random_range(0..1000u32);
        let name = kind.name(tier, n);
        let floors = service_floors(kind, tier);
        let parking = match kind {
            ServiceKind::Stadium => [20, 60, 140][tier.index()],
            ServiceKind::Health | ServiceKind::School => [4, 10, 24][tier.index()],
            _ => [2, 4, 8][tier.index()],
        };
        let id = self.place_special(lot_id, name, kind.jobs(tier), parking, |b| {
            b.service = Some(ServiceSite { kind, tier });
            b.tier = tier;
            b.floors = floors;
        });
        self.services.rubble.retain(|r| r.0 != lot_id);
        self.ensure_units(id);
        self.refresh_coverage();
        Ok(id)
    }

    /// Give a station the vehicles its tier runs.
    fn ensure_units(&mut self, station: BuildingId) {
        let Some((kind, tier, pos)) = self.buildings.get(station).and_then(|b| b.service.map(|s| (s.kind, s.tier, self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.car_point)))) else { return };
        let Some(uk) = kind.unit_kind() else { return };
        let have = self.services.units.values().filter(|u| u.station == station).count();
        for k in have..kind.units(tier) {
            self.services.units.insert(Unit { kind: uk, station, number: k as u8 + 1, state: UnitState::Idle, car: None, incident: None, pos, heading: 0.0, hospital: None, patrol_until: 0, leaving: false });
        }
    }

    /// Vehicles a station can put on the road with the staff it has.
    pub fn crewed_units(&self, station: BuildingId) -> usize {
        let Some(b) = self.buildings.get(station) else { return 0 };
        let Some(s) = b.service else { return 0 };
        let Some(uk) = s.kind.unit_kind() else { return 0 };
        if b.is_derelict() || b.construction.is_some() {
            return 0;
        }
        s.kind.units(s.tier).min(b.workers.len() / uk.crew())
    }

    // ── coverage ───────────────────────────────────────────────────────────

    /// Where a lot meets the street network: the car point, or the
    /// entrance on its frontage for lots on a path.
    fn lot_access(&self, lot: &Lot) -> Option<(SegmentId, Vec2)> {
        let (seg, p) = match lot.car_segment {
            Some(s) => (s, lot.car_point),
            None => (lot.front_segment, lot.entrance()),
        };
        self.roads.try_segment(seg).map(|_| (seg, p))
    }

    fn coverage_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.roads.version.hash(&mut h);
        self.lots.len().hash(&mut h);
        for (id, b) in self.buildings.iter() {
            if let Some(s) = b.service {
                if s.kind.coverage_index().is_some() {
                    (id, s.tier.index(), b.is_derelict(), b.lot).hash(&mut h);
                }
            }
        }
        h.finish()
    }

    /// Coverage of every lot by every covering service: the best road
    /// budget left (a station's radius minus the road distance) over all
    /// stations of the kind, faded over the last `COVERAGE_FADE` metres.
    pub fn refresh_coverage(&mut self) {
        let n = self.roads.nodes.len();
        let mut out: HashMap<LotId, [f32; 4]> = HashMap::new();
        let mut sources: [Vec<(SegmentId, Vec2, f32)>; 4] = Default::default();
        for b in self.buildings.values() {
            let Some(s) = b.service else { continue };
            let Some(k) = s.kind.coverage_index() else { continue };
            if b.is_derelict() || b.construction.is_some() {
                continue;
            }
            if let Some((seg, p)) = self.lots.get(b.lot).and_then(|l| self.lot_access(l)) {
                sources[k].push((seg, p, s.kind.radius(s.tier)));
            }
        }
        for (k, src) in sources.iter().enumerate() {
            if src.is_empty() {
                continue;
            }
            let mut best = vec![f32::NEG_INFINITY; n];
            let mut heap = BinaryHeap::new();
            for &(seg, p, r) in src {
                let s = self.roads.segment(seg);
                for node in [s.a, s.b] {
                    let budget = r - p.distance(self.roads.node(node).pos);
                    if budget > best[node.0 as usize] {
                        best[node.0 as usize] = budget;
                        heap.push(Budget(budget, node.0));
                    }
                }
            }
            while let Some(Budget(b, node)) = heap.pop() {
                if b < best[node as usize] || b <= 0.0 {
                    continue;
                }
                let here = self.roads.node(NodeId(node)).pos;
                for (nb, sid) in self.roads.neighbors(NodeId(node)) {
                    if self.roads.segment(sid).is_rail() {
                        continue;
                    }
                    let nbudget = b - here.distance(self.roads.node(nb).pos);
                    if nbudget > best[nb.0 as usize] {
                        best[nb.0 as usize] = nbudget;
                        heap.push(Budget(nbudget, nb.0));
                    }
                }
            }
            for (lid, lot) in self.lots.iter() {
                let Some((seg, p)) = self.lot_access(lot) else { continue };
                let s = self.roads.segment(seg);
                let mut rem = [s.a, s.b].iter().map(|nd| best[nd.0 as usize] - p.distance(self.roads.node(*nd).pos)).fold(f32::NEG_INFINITY, f32::max);
                for &(sseg, sp, r) in src {
                    if sseg == seg {
                        rem = rem.max(r - p.distance(sp));
                    }
                }
                let cov = (rem / COVERAGE_FADE).clamp(0.0, 1.0);
                if cov > 0.0 {
                    out.entry(lid).or_insert([0.0; 4])[k] = cov;
                }
            }
        }
        self.services.coverage = out;
        self.services.coverage_key = Some(self.coverage_signature());
        // Share of homes covered, per kind.
        let homes: Vec<LotId> = self.buildings.values().filter(|b| b.homes > 0 && b.construction.is_none()).map(|b| b.lot).collect();
        for k in 0..4 {
            let covered = homes.iter().filter(|l| self.services.coverage.get(l).is_some_and(|c| c[k] >= COVERED)).count();
            self.services.stats.coverage[k] = if homes.is_empty() { 0.0 } else { covered as f32 / homes.len() as f32 };
        }
    }

    /// Coverage of the lot nearest `p` by a covering service kind.
    pub fn coverage_at(&self, p: Vec2, kind: ServiceKind) -> f32 {
        self.lots.iter().filter(|(_, l)| l.usable()).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map_or(0.0, |(id, _)| self.services.coverage_of(id, kind))
    }

    // ── starting incidents ─────────────────────────────────────────────────

    fn open_incident(&mut self, kind: IncidentKind, building: BuildingId, citizen: Option<CitizenId>, suspect: Option<CitizenId>, report_delay: u64) -> u32 {
        let tick = self.clock.tick;
        let id = self.services.next_id;
        self.services.next_id += 1;
        let pos = self.building_lot(building).map_or(Vec2::ZERO, |l| l.center);
        let work_left = match kind {
            IncidentKind::Fire => 0.0,
            IncidentKind::Crime => CRIME_WORK,
            IncidentKind::Medical => MEDICAL_WORK,
            IncidentKind::Crash => CRASH_WORK,
        };
        self.services.incidents.push(Incident {
            id,
            kind,
            building,
            pos,
            citizen,
            suspect,
            caller: None,
            dispatcher: None,
            stage: Stage::Unreported,
            started: tick,
            report_at: tick + report_delay,
            reported: None,
            dispatch_at: None,
            dispatched: None,
            arrived: None,
            resolved: None,
            unit: None,
            intensity: if kind == IncidentKind::Fire { 0.15 } else { 0.0 },
            work_left,
            outcome: None,
            place: self.buildings.get(building).map(|b| b.name.clone()).unwrap_or_default(),
            crime: None,
            outside: false,
            arrive_at: None,
            fee: 0.0,
            unit_name: None,
            people: Vec::new(),
            damage: Vec::new(),
            on_road: false,
            chase: None,
        });
        if let Some(n) = self.services.stats.incidents_today.get_mut(kind.index()) {
            *n += 1;
        }
        self.events.push(SimEvent::IncidentStarted(id));
        id
    }

    /// Set a building alight. None if it cannot burn or already does.
    pub fn start_fire(&mut self, bid: BuildingId) -> Option<u32> {
        let b = self.buildings.get(bid)?;
        if b.construction.is_some() || b.land_use == LandUse::Park || b.rail.is_some() || self.services.is_burning(bid) {
            return None;
        }
        let occupied = !b.residents.is_empty() || !b.workers.is_empty() || b.visitors > 0;
        let name = b.name.clone();
        let people: Vec<CitizenId> = b.residents.iter().chain(b.workers.iter()).copied().collect();
        let delay = if occupied { self.srng().random_range(2..8) } else { self.srng().random_range(10..40) };
        let tick = self.clock.tick;
        for cid in people {
            if let Some(c) = self.citizens.get_mut(cid) {
                c.remember(tick, format!("{name} caught fire."));
                self.concerns.heard(cid, c, crate::concerns::Concern::Safety, 0.6);
                c.mood = (c.mood - 0.08).max(0.0);
            }
        }
        Some(self.open_incident(IncidentKind::Fire, bid, None, None, delay))
    }

    /// Hook for a crime that something else decided on (a criminal citizen,
    /// a gang): what, where, who did it and to whom. Somebody calls it in a
    /// few minutes later and the police answer like any other job. Returns
    /// the incident id; arrests show as `Outcome::Arrest`, `SimEvent::Arrested`
    /// and `Services::arrests_of`.
    pub fn report_crime(&mut self, kind: CrimeKind, at: BuildingId, suspect: Option<CitizenId>, victim: Option<CitizenId>) -> Option<u32> {
        if !self.buildings.contains_key(at) {
            return None;
        }
        let delay = self.srng().random_range(3..20);
        let id = self.open_incident(IncidentKind::Crime, at, victim, suspect, delay);
        let names: Vec<(CitizenId, String, &str)> = [(victim, "victim"), (suspect, "suspect")].into_iter().filter_map(|(c, role)| c.and_then(|c| self.citizens.get(c).map(|x| (c, x.name.clone(), role)))).collect();
        if let Some(i) = self.services.incident_mut(id) {
            i.crime = Some(kind);
            for (c, n, role) in names {
                i.involve(c, &n, role);
            }
        }
        Some(id)
    }

    /// [driving] A bad crash on the road at `at`: the police come to clear
    /// it (and arrest `at_fault`, if given, when they can), and every one of
    /// `injured` waits there for an ambulance. `near` names the place.
    /// `caller` is whoever saw it and calls 911. Returns the crash incident
    /// and the medical ones.
    pub fn report_crash(&mut self, at: Vec2, near: BuildingId, drivers: &[CitizenId], injured: &[CitizenId], at_fault: Option<CitizenId>, caller: Option<CitizenId>) -> Option<(u32, Vec<u32>)> {
        if !self.buildings.contains_key(near) {
            return None;
        }
        // Out past the edge of the map (the highway runs on for kilometres)
        // a crash is the next town's business: their police and ambulances
        // take it, and it is never a call of ours. The hurt are patched up
        // and go on; the wreck is towed in its own time.
        if at.x.abs().max(at.y.abs()) > crate::landscape::MAP_EDGE {
            let tick = self.clock.tick;
            for &p in injured {
                if let Some(c) = self.citizens.get_mut(p) {
                    c.remember(tick, "Crashed out past the city. The next town's paramedics patched me up.");
                    c.next_decision_tick = c.next_decision_tick.min(tick + 60);
                }
            }
            return None;
        }
        // A crash where a crash scene is already open (the same junction, a
        // pile-up) joins that scene: one police car clears both wrecks.
        let joined = self.services.active().filter(|i| i.kind == IncidentKind::Crash && i.pos.distance(at) < CRASH_SCENE_JOIN).map(|i| i.id).next();
        let delay = self.srng().random_range(1..5);
        let id = match joined {
            Some(id) => id,
            None => self.open_incident(IncidentKind::Crash, near, None, at_fault, delay),
        };
        let work = if at_fault.is_some() {
            CRASH_WORK_ARREST
        } else if !injured.is_empty() {
            CRASH_WORK
        } else {
            CRASH_WORK_CLEAR
        };
        if let Some(i) = self.services.incident_mut(id) {
            // A joined scene takes as long as its longest case.
            i.work_left = if joined.is_some() { i.work_left.max(work) } else { work };
            if joined.is_some() && i.suspect.is_none() {
                i.suspect = at_fault;
            }
        }
        let names: Vec<(CitizenId, String, &str)> = drivers
            .iter()
            .map(|&c| (c, if Some(c) == at_fault { "driver at fault" } else { "driver" }))
            .chain(injured.iter().map(|&c| (c, "injured")))
            .filter_map(|(c, role)| self.citizens.get(c).map(|x| (c, x.name.clone(), role)))
            .collect();
        if let Some(i) = self.services.incident_mut(id) {
            if joined.is_none() {
                i.pos = at;
                i.on_road = true;
                i.caller = caller;
            }
            for (c, n, role) in names {
                i.involve(c, &n, role);
            }
        }
        let mut medical = Vec::new();
        for &p in injured {
            let Some(name) = self.citizens.get(p).map(|c| c.name.clone()) else { continue };
            let m = self.open_incident(IncidentKind::Medical, near, Some(p), None, delay);
            if let Some(i) = self.services.incident_mut(m) {
                i.pos = at;
                i.on_road = true;
                i.caller = caller;
                i.involve(p, &name, "patient");
            }
            medical.push(m);
        }
        Some((id, medical))
    }

    /// A crime at a building: a victim from whoever is there, a suspect
    /// from the town's idle and unhappy, if any.
    pub fn start_crime(&mut self, bid: BuildingId) -> Option<u32> {
        let b = self.buildings.get(bid)?;
        if self.services.active().any(|i| i.kind == IncidentKind::Crime && i.building == bid) {
            return None;
        }
        let name = b.name.clone();
        let regulars: Vec<CitizenId> = b.residents.iter().chain(b.workers.iter()).copied().collect();
        let present: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.inside == Some(bid)).map(|(id, _)| id).collect();
        let pool: Vec<CitizenId> = if present.is_empty() { regulars } else { present };
        let victim = pool.choose(self.srng()).copied();
        let suspects: Vec<CitizenId> = self.citizens.iter().filter(|(id, c)| c.work.is_none() && c.can_work() && c.mood < 0.5 && Some(*id) != victim && !self.services.in_custody(*id)).map(|(id, _)| id).collect();
        let suspect = if self.srng().random_bool(0.7) { suspects.choose(self.srng()).copied() } else { None };
        let tick = self.clock.tick;
        if let Some(v) = victim.and_then(|v| self.citizens.get_mut(v)) {
            v.remember(tick, format!("Got robbed at {name}."));
            self.concerns.heard(victim.unwrap_or_default(), v, crate::concerns::Concern::Safety, 0.6);
            v.mood = (v.mood - 0.1).max(0.0);
            v.thought = "Somebody just robbed me.".to_string();
        }
        let kind = *[CrimeKind::Robbery, CrimeKind::Theft, CrimeKind::Assault, CrimeKind::Vandalism].choose(self.srng()).unwrap();
        self.report_crime(kind, bid, suspect, victim)
    }

    /// A citizen indoors is taken ill and stays put until help comes.
    pub fn fall_ill(&mut self, cid: CitizenId) -> Option<u32> {
        let c = self.citizens.get(cid)?;
        let bid = c.inside?;
        if !self.buildings.contains_key(bid) || self.is_patient(cid) {
            return None;
        }
        let tick = self.clock.tick;
        let delay = self.srng().random_range(1..6);
        let c = &mut self.citizens[cid];
        c.thought = "I don't feel well at all.".to_string();
        c.next_decision_tick = c.next_decision_tick.max(tick + MEDICAL_GIVE_UP + 120);
        c.mood = (c.mood - 0.05).max(0.0);
        let name = self.citizens[cid].name.clone();
        let id = self.open_incident(IncidentKind::Medical, bid, Some(cid), None, delay);
        if let Some(i) = self.services.incident_mut(id) {
            i.involve(cid, &name, "patient");
        }
        Some(id)
    }

    /// Ill and waiting, or in a hospital bed.
    pub fn is_patient(&self, cid: CitizenId) -> bool {
        self.services.active().any(|i| i.kind == IncidentKind::Medical && i.citizen == Some(cid)) || self.services.admissions.iter().any(|a| a.citizen == cid)
    }

    // ── the hourly roll ────────────────────────────────────────────────────

    /// Services hire: each with open jobs takes up to two people an hour,
    /// the jobless first, else someone on a lower wage elsewhere (a public
    /// job pays well), so a fully employed town still staffs its stations.
    fn recruit_staff(&mut self) {
        let posts: Vec<(BuildingId, f32)> = self.buildings.iter().filter(|(_, b)| b.service.is_some() && b.construction.is_none() && !b.is_derelict() && b.free_jobs() > 0).map(|(id, b)| (id, b.wage())).collect();
        for (bid, wage) in posts {
            for _ in 0..2 {
                if self.buildings[bid].free_jobs() == 0 {
                    break;
                }
                // Round-the-clock posts hire for the thinnest shift first.
                let want = self.thinnest_shift(bid).map(|(s, _)| s);
                let b = &self.buildings[bid];
                let jobless = self
                    .citizens
                    .iter()
                    .filter(|(_, c)| c.work.is_none() && c.can_work())
                    .find(|(_, c)| want.is_none_or(|w| b.shift_for(c.seed).open == w))
                    .or_else(|| self.citizens.iter().find(|(_, c)| c.work.is_none() && c.can_work()))
                    .map(|(id, _)| id);
                let poach = || {
                    self.citizens
                        .iter()
                        .filter(|(_, c)| c.can_work())
                        .filter_map(|(id, c)| c.work.and_then(|w| self.buildings.get(w)).filter(|w| !w.is_special() && w.construction.is_none() && w.wage() < wage).map(|_| id))
                        .next()
                };
                let Some(cid) = jobless.or_else(poach) else { break };
                if let Some(old) = self.citizens[cid].work.and_then(|w| self.buildings.get_mut(w)) {
                    old.workers.retain(|x| *x != cid);
                }
                self.assign_job(cid, bid);
            }
        }
    }

    pub(super) fn hourly_services(&mut self) {
        crate::prof_scope!("hourly_services");
        self.recruit_staff();
        self.balance_rosters();
        if !self.config.emergencies {
            let tick = self.clock.tick;
            let citizens = &self.citizens;
            self.services.admissions.retain(|a| a.until > tick && citizens.get(a.citizen).is_some_and(|c| c.inside == Some(a.hospital)));
            self.refresh_coverage();
            return;
        }
        let hour = self.clock.hour();
        let night = !(5..20).contains(&hour);
        let pop = self.citizens.len().max(1);
        let unemployment = self.citizens.values().filter(|c| c.work.is_none()).count() as f32 / pop as f32;
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for bid in ids {
            let b = &self.buildings[bid];
            if b.construction.is_some() || b.land_use == LandUse::Park || b.service.is_some_and(|s| s.kind == ServiceKind::Monument) {
                continue;
            }
            let lot = b.lot;
            let lv = self.lots.get(lot).map_or(b.land_value, |l| self.land_value.at(l.center));
            let lu_fire = match b.land_use {
                LandUse::Industrial => 2.0,
                LandUse::Entertainment => 1.4,
                _ => 1.0,
            } * if b.is_derelict() { 2.0 } else { 1.0 };
            let p_fire = FIRE_RISK[b.tier.index()] * (1.0 + 2.0 * (1.0 - b.condition)) * lu_fire * (1.0 - FIRE_COVER_CUT * self.services.coverage_of(lot, ServiceKind::Fire));
            let occupied = !b.residents.is_empty() || !b.workers.is_empty() || b.business.is_some();
            let lu_crime = match b.land_use {
                LandUse::Entertainment => 1.5,
                LandUse::Commercial => 1.2,
                _ => 1.0,
            };
            let p_crime = CRIME_RISK * (1.0 + 3.0 * unemployment) * (1.4 - lv).max(0.3) * if night { 1.6 } else { 1.0 } * lu_crime * (1.0 - CRIME_COVER_CUT * self.services.coverage_of(lot, ServiceKind::Police));
            if self.srng().random::<f32>() < p_fire {
                self.start_fire(bid);
            }
            if occupied && self.srng().random::<f32>() < p_crime {
                self.start_crime(bid);
            }
        }
        // Illness: anyone indoors, more for the old and the hungry, less
        // where a hospital is near home.
        let cids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in cids {
            let c = &self.citizens[cid];
            let Some(inside) = c.inside else { continue };
            // Small children and the old fall ill most.
            let age = c.age as f32;
            let frailty = if age < 6.0 { 1.8 } else if age >= 67.0 { 1.5 + (age - 67.0) / 15.0 } else { 0.6 + age / 80.0 };
            let hunger = ((0.35 - c.satiety) / 0.35).clamp(0.0, 1.0);
            let home_lot = c.home.and_then(|h| self.buildings.get(h)).or_else(|| self.buildings.get(inside)).map(|b| b.lot);
            let cov = home_lot.map_or(0.0, |l| self.services.coverage_of(l, ServiceKind::Health));
            let p = SICK_RISK * frailty * (1.0 + 4.0 * hunger) * (1.0 - SICK_COVER_CUT * cov);
            if self.srng().random::<f32>() < p && !self.is_patient(cid) {
                self.fall_ill(cid);
            }
        }
        // Discharged or wandered off: free the bed.
        let tick = self.clock.tick;
        let citizens = &self.citizens;
        self.services.admissions.retain(|a| a.until > tick && citizens.get(a.citizen).is_some_and(|c| c.inside == Some(a.hospital)));
        self.services.rubble.retain(|r| r.2 > tick);
        self.refresh_coverage();
    }

    // ── every tick ─────────────────────────────────────────────────────────

    pub(super) fn step_services(&mut self) {
        let tick = self.clock.tick;
        if tick % 10 == 3 {
            // Stations come and go; roads change under them.
            let lost: Vec<UnitId> = self.services.units.iter().filter(|(_, u)| !self.station_alive(u.station)).map(|(id, _)| id).collect();
            for id in lost {
                self.station_lost(id);
            }
            let stations: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.service.is_some_and(|s| s.kind.unit_kind().is_some())).map(|(id, _)| id).collect();
            for s in stations {
                self.ensure_units(s);
            }
            if self.services.coverage_key != Some(self.coverage_signature()) {
                self.refresh_coverage();
            }
        }
        // [emergency] Car chases before lost units are looked for.
        self.step_chases();
        self.recover_lost_units();
        // Lights and sirens pull up short of the kerb and the crew is out
        // while the car is still rolling to a stop: on scene within
        // `PULL_UP` metres of the stop (the last stretch of braking took a
        // minute or two of clock).
        let pulling_up: Vec<CarId> = self
            .services
            .units
            .values()
            .filter(|u| u.state == UnitState::Responding)
            .filter_map(|u| u.car)
            .filter(|c| self.traffic.cars.get(*c).is_some_and(|car| car.idx + 1 == car.route.len() && car.s_end - car.s < PULL_UP))
            .collect();
        for car in pulling_up {
            self.unit_car_arrived(car);
        }
        let active: Vec<u32> = self.services.active().map(|i| i.id).collect();
        for id in active {
            self.step_incident(id);
        }
        self.dispatch_queue();
        if tick % 5 == 2 {
            self.step_patrols();
        }
        if self.clock.minute_of_day() == EVENT_MINUTE {
            self.start_stadium_events();
        }
        if self.clock.minute_of_day() == SCHOOL_MINUTE {
            self.send_children_to_school();
        }
        // Pupils stay until the school closes.
        if tick % 10 == 1 {
            self.step_custody();
        }
        if tick % 10 == 5 {
            let hour = self.clock.hour() as u8;
            let buildings = &self.buildings;
            for c in self.citizens.values_mut() {
                if let Activity::Visiting(b) = c.activity {
                    if c.goes_to_school() && buildings.get(b).and_then(|x| x.service).is_some_and(|s| s.kind == ServiceKind::School && s.kind.hours().contains(hour)) {
                        c.next_decision_tick = c.next_decision_tick.max(tick + 15);
                    }
                }
            }
        }
        // Keep the record short.
        let resolved = self.services.incidents.iter().filter(|i| !i.active()).count();
        if resolved > KEEP_RESOLVED {
            let mut drop = resolved - KEEP_RESOLVED;
            self.services.incidents.retain(|i| {
                if !i.active() && drop > 0 {
                    drop -= 1;
                    false
                } else {
                    true
                }
            });
        }
        let s = &mut self.services;
        s.by_car = s.units.iter().filter_map(|(id, u)| u.car.map(|c| (c, id))).collect();
        s.stats.units = s.units.len() as u32;
        s.stats.units_busy = s.units.values().filter(|u| u.busy()).count() as u32;
        s.stats.on_patrol = s.units.values().filter(|u| u.state == UnitState::Patrol).count() as u32;
        s.stats.fires_burning = s.active().filter(|i| i.kind == IncidentKind::Fire).count() as u32;
        s.stats.waiting_for_ambulance = s.active().filter(|i| i.kind == IncidentKind::Medical && matches!(i.stage, Stage::Unreported | Stage::Reported | Stage::Dispatched)).count() as u32;
        s.stats.in_hospital = s.admissions.len() as u32;
    }

    /// Units free for a call, per incident kind (`IncidentKind::index`): in
    /// the station with a crew to spare, on patrol or standby, or driving home.
    fn free_units(&self) -> [usize; 3] {
        let out = self.units_out();
        let mut n = [0; 3];
        for u in self.services.units.values().filter(|u| self.unit_available(u, &out)) {
            n[u.kind as usize] += 1;
        }
        n
    }

    /// Units out of the station (crews aboard), per station.
    fn units_out(&self) -> HashMap<BuildingId, usize> {
        let mut out: HashMap<BuildingId, usize> = HashMap::new();
        for u in self.services.units.values().filter(|u| u.out()) {
            *out.entry(u.station).or_default() += 1;
        }
        out
    }

    /// Free for the next call: a unit in the station counts only while the
    /// station has a crew for one more vehicle; units already out have theirs.
    fn unit_available(&self, u: &Unit, out: &HashMap<BuildingId, usize>) -> bool {
        // A unit leaving town with its station gone takes no more calls.
        if u.leaving || !self.station_alive(u.station) {
            return false;
        }
        match u.state {
            UnitState::Idle => out.get(&u.station).copied().unwrap_or(0) < self.crewed_units(u.station),
            UnitState::Patrol | UnitState::Returning => true,
            _ => false,
        }
    }

    /// Whether the city has a crewed station for the kind.
    fn city_has(&self, kind: IncidentKind) -> bool {
        self.services.units.values().any(|u| u.kind == kind.unit() && self.crewed_units(u.station) > 0)
    }

    /// Every tick: the calls past their dispatch time, most urgent first
    /// (`Incident::priority`, then call order), each get the nearest free
    /// unit. A call that finds none waits in the queue and goes out the tick
    /// a unit clears; a violent crime may pull a car off a theft; a call left
    /// waiting `MUTUAL_AID_WAIT` with every unit busy goes to the next town.
    /// Also keeps the queue and saturation figures.
    fn dispatch_queue(&mut self) {
        let tick = self.clock.tick;
        let mut queue: Vec<(u8, u64, u32, IncidentKind)> = self
            .services
            .active()
            .filter(|i| i.stage == Stage::Reported && i.dispatch_at.is_some_and(|t| tick >= t))
            .map(|i| (i.priority(), i.reported.unwrap_or(i.started), i.id, i.kind))
            .collect();
        queue.sort_by_key(|q| (q.0, q.1, q.2));
        let mut free = self.free_units();
        let mut queued = [0u32; 3];
        for (prio, reported, id, kind) in queue {
            let k = kind.slot();
            let first = self.services.incident(id).and_then(|i| i.dispatch_at) == Some(tick);
            // A call no free unit could reach is retried every five minutes.
            let retry = first || tick % 5 == 0 || !self.services.route_failed.contains_key(&id);
            if (free[k] > 0 || first || !self.city_has(kind)) && retry {
                if self.try_dispatch(id) {
                    self.services.route_failed.remove(&id);
                    free = self.free_units();
                    continue;
                }
                if free[k] > 0 {
                    // Free units, but no road from any of them: a crash on
                    // the highway beyond the city's streets, a cut-off block.
                    // The next town's (or the highway patrol's) units take it.
                    self.services.route_failed.insert(id, tick);
                    if self.services.incident(id).and_then(|i| i.dispatcher).is_some() {
                        self.services.stats.unreachable_today += 1;
                        self.send_from_out_of_town(id, MUTUAL_AID_TRAVEL, true);
                        continue;
                    }
                }
            }
            if free[k] == 0 && prio == 0 && kind == IncidentKind::Crime && self.divert_for(id) {
                self.services.stats.diverted_today += 1;
                free = self.free_units();
                continue;
            }
            let dispatcher = self.services.incident(id).and_then(|i| i.dispatcher);
            if free[k] == 0 && dispatcher.is_some() && tick.saturating_sub(reported) >= MUTUAL_AID_WAIT && self.city_has(kind) {
                self.services.stats.mutual_aid_today += 1;
                self.send_from_out_of_town(id, MUTUAL_AID_TRAVEL, true);
                continue;
            }
            queued[k] += 1;
        }
        let has = IncidentKind::ALL.map(|kind| self.city_has(kind));
        let st = &mut self.services.stats;
        for k in 0..3 {
            st.queued[k] = queued[k];
            st.queue_max_today[k] = st.queue_max_today[k].max(queued[k]);
            if has[k] && free[k] == 0 {
                st.saturated_since[k].get_or_insert(tick);
                st.saturated_min_today[k] += 1;
            } else {
                st.saturated_since[k] = None;
            }
        }
    }

    /// A violent crime with every police car busy: pull the nearest car that
    /// is still on its way to a theft or vandalism; that call goes back in
    /// the queue.
    fn divert_for(&mut self, id: u32) -> bool {
        let Some(at) = self.services.incident(id).map(|i| i.pos) else { return false };
        let Some(to) = self.incident_access(id) else { return false };
        let mut cands: Vec<(f32, UnitId, u32)> = Vec::new();
        for (uid, u) in self.services.units.iter() {
            if u.kind != UnitKind::PoliceCar || u.state != UnitState::Responding {
                continue;
            }
            let Some(other) = u.incident.and_then(|o| self.services.incident(o)) else { continue };
            if other.priority() < 2 || other.stage != Stage::Dispatched {
                continue;
            }
            let p = u.car.and_then(|c| self.traffic.cars.get(c)).map_or(u.pos, |c| c.pos);
            cands.push((p.distance(at), uid, other.id));
        }
        cands.sort_by(|a, b| a.0.total_cmp(&b.0));
        let tick = self.clock.tick;
        for (_, uid, other) in cands {
            if !self.send_unit(uid, id, to) {
                continue;
            }
            if let Some(o) = self.services.incident_mut(other) {
                o.stage = Stage::Reported;
                o.unit = None;
                o.dispatched = None;
                o.unit_name = None;
                o.dispatch_at = Some(tick);
            }
            return true;
        }
        false
    }

    /// Where a unit pulls up for an incident.
    fn incident_access(&self, id: u32) -> Option<(SegmentId, Vec2)> {
        let inc = self.services.incident(id)?;
        // Street incidents (crashes, the hurt in them, takeovers): at the spot.
        let on_street = if inc.on_road { self.roads.nearest_street(inc.pos).map(|(s, _, q)| (s, q)) } else { None };
        on_street.or_else(|| self.building_lot(inc.building).and_then(|l| self.lot_access(l))).or_else(|| self.roads.nearest_street(inc.pos).map(|(s, _, q)| (s, q)))
    }

    /// Take a unit's car off the road, leaving the unit where the car was.
    pub(super) fn pull_off_road(&mut self, uid: UnitId) {
        let Some(c) = self.services.units.get_mut(uid).and_then(|u| u.car.take()) else { return };
        if let Some(car) = self.traffic.remove_car(c) {
            let u = &mut self.services.units[uid];
            u.pos = car.pos;
            u.heading = car.heading;
        }
        self.services.by_car.remove(&c);
    }

    // ── patrols and standby ────────────────────────────────────────────────

    /// Police cars patrol and ambulances post at standby points while free;
    /// a parked unit moves to its next beat when its stay is up. A unit
    /// whose station has lost the crew for it goes home.
    fn step_patrols(&mut self) {
        let tick = self.clock.tick;
        let ids: Vec<UnitId> = self.services.units.iter().filter(|(_, u)| matches!(u.kind, UnitKind::PoliceCar | UnitKind::Ambulance)).map(|(id, _)| id).collect();
        for uid in ids {
            let out = self.units_out();
            let u = &self.services.units[uid];
            let (station, state, parked, until) = (u.station, u.state, u.car.is_none(), u.patrol_until);
            let crewed = self.crewed_units(station);
            let n_out = out.get(&station).copied().unwrap_or(0);
            match state {
                UnitState::Idle if n_out < crewed => {
                    self.start_patrol(uid);
                }
                UnitState::Patrol if n_out > crewed => self.return_to_station(uid),
                UnitState::Patrol if parked && tick >= until => {
                    if !self.start_patrol(uid) {
                        self.return_to_station(uid);
                    }
                }
                _ => {}
            }
        }
    }

    /// Drive to a new beat within the station's district (police: crime
    /// hot spots and shopping streets; ambulances: where people live), away
    /// from the kind's other units on patrol. False if there is nowhere to go.
    fn start_patrol(&mut self, uid: UnitId) -> bool {
        let Some(u) = self.services.units.get(uid) else { return false };
        let (station, kind) = (u.station, u.kind);
        let here = u.car.and_then(|c| self.traffic.cars.get(c)).map_or(u.pos, |c| c.pos);
        let Some((centre, radius)) = self.buildings.get(station).and_then(|b| Some((self.lots.get(b.lot)?.center, b.service.map(|s| s.kind.radius(s.tier))?))) else { return false };
        let reach = radius * PATROL_REACH;
        let others: Vec<Vec2> = self
            .services
            .units
            .iter()
            .filter(|(id, o)| *id != uid && o.kind == kind && o.state == UnitState::Patrol)
            .map(|(_, o)| o.car.and_then(|c| self.traffic.cars.get(c)).map_or(o.pos, |c| c.pos))
            .collect();
        // Where this kind of call came from over the last two days.
        let since = self.clock.tick.saturating_sub(2 * 1440);
        let recent: Vec<Vec2> = self.services.incidents.iter().filter(|i| i.kind.unit() == kind && i.started >= since).map(|i| i.pos).collect();
        let mut cands: Vec<(f32, LotId)> = Vec::new();
        for b in self.buildings.values() {
            if b.construction.is_some() || b.land_use == LandUse::Park {
                continue;
            }
            let Some(l) = self.lots.get(b.lot) else { continue };
            if l.center.distance(centre) > reach || l.center.distance(here) < 60.0 {
                continue;
            }
            let calls = recent.iter().filter(|p| p.distance(l.center) < 160.0).count() as f32;
            let base = match kind {
                UnitKind::PoliceCar => 1.0 + 4.0 * self.society.crime.heat_at(l.center, 120.0) + 2.0 * calls + if matches!(b.land_use, LandUse::Commercial | LandUse::Entertainment) { 1.0 } else { 0.0 },
                _ => 0.2 + b.residents.len() as f32 / 10.0 + 2.0 * calls,
            };
            let crowd = others.iter().filter(|p| p.distance(l.center) < 200.0).count() as f32;
            cands.push((base / (1.0 + 3.0 * crowd), b.lot));
        }
        let total: f32 = cands.iter().map(|c| c.0).sum();
        if cands.is_empty() || total <= 0.0 {
            return false;
        }
        let mut roll = self.srng().random::<f32>() * total;
        let mut pick = cands[cands.len() - 1].1;
        for (w, lot) in &cands {
            roll -= w;
            if roll <= 0.0 {
                pick = *lot;
                break;
            }
        }
        let Some(to) = self.lots.get(pick).and_then(|l| self.lot_access(l)) else { return false };
        self.pull_off_road(uid);
        if !self.drive_unit(uid, to, false) {
            return false;
        }
        let u = &mut self.services.units[uid];
        u.state = UnitState::Patrol;
        u.incident = None;
        u.hospital = None;
        true
    }

    /// Off patrol and back to the station.
    fn return_to_station(&mut self, uid: UnitId) {
        if self.services.units.get(uid).is_some_and(|u| !self.station_alive(u.station)) {
            self.unit_leaves_town(uid);
            return;
        }
        self.pull_off_road(uid);
        let Some(u) = self.services.units.get_mut(uid) else { return };
        u.incident = None;
        u.hospital = None;
        let station = u.station;
        let to = self.building_lot(station).and_then(|l| self.lot_access(l));
        match to {
            Some(to) if self.drive_unit(uid, to, false) => self.services.units[uid].state = UnitState::Returning,
            _ => self.park_at_station(uid),
        }
    }

    // ── shifts ─────────────────────────────────────────────────────────────

    /// For a round-the-clock emergency post: the shift (by its opening hour)
    /// with the fewest workers, and how many it has. None for other buildings.
    fn thinnest_shift(&self, bid: BuildingId) -> Option<(u8, usize)> {
        let b = self.buildings.get(bid)?;
        let s = b.service?;
        if !matches!(s.kind, ServiceKind::Police | ServiceKind::Fire | ServiceKind::Health | ServiceKind::Dispatch | ServiceKind::Jail) {
            return None;
        }
        let shifts: Vec<u8> = (0..3u64).map(|k| s.kind.hours().shift(k).open).collect();
        shifts.iter().map(|&open| (open, b.workers.iter().filter(|w| self.citizens.get(**w).is_some_and(|c| b.shift_for(c.seed).open == open)).count())).min_by_key(|x| x.1)
    }

    /// Emergency posts keep every shift manned: once an hour, a post with an
    /// empty shift and a doubled-up one swaps a worker with someone whose
    /// hours fit the empty shift, from an ordinary job paying less (they take
    /// the public job, ours takes theirs). Hired at random, a 911 centre of
    /// three could leave the evening with nobody at the desk.
    fn balance_rosters(&mut self) {
        let posts: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.service.is_some() && b.construction.is_none() && !b.is_derelict() && b.workers.len() >= 3).map(|(id, _)| id).collect();
        for bid in posts {
            let Some((want, 0)) = self.thinnest_shift(bid) else { continue };
            let b = &self.buildings[bid];
            let wage = b.wage();
            // Ordered, so ties between equal shifts break the same way every run.
            let mut by_shift: std::collections::BTreeMap<u8, Vec<CitizenId>> = std::collections::BTreeMap::new();
            for &w in &b.workers {
                if let Some(c) = self.citizens.get(w) {
                    by_shift.entry(b.shift_for(c.seed).open).or_default().push(w);
                }
            }
            let Some(spare) = by_shift.values().filter(|v| v.len() >= 2).max_by_key(|v| v.len()).and_then(|v| v.last().copied()) else { continue };
            let cand = self
                .citizens
                .iter()
                .filter(|(_, c)| c.can_work() && b.shift_for(c.seed).open == want)
                .filter_map(|(id, c)| c.work.filter(|w| self.buildings.get(*w).is_some_and(|x| !x.is_special() && x.construction.is_none() && x.wage() < wage)).map(|w| (id, w)))
                .next();
            let Some((cid, old)) = cand else { continue };
            self.buildings[bid].workers.retain(|x| *x != spare);
            self.buildings[old].workers.retain(|x| *x != cid);
            self.assign_job(cid, bid);
            self.assign_job(spare, old);
        }
    }


    /// A unit whose car vanished (roads rebuilt, a save loaded) carries on
    /// as best it can: a responder is sent again, a returning unit is home,
    /// a patient on the way is delivered.
    fn recover_lost_units(&mut self) {
        let lost: Vec<UnitId> = self.services.units.iter().filter(|(_, u)| u.car.is_some_and(|c| !self.traffic.cars.contains_key(c))).map(|(id, _)| id).collect();
        for uid in lost {
            // Leaving town with no station: its car was taken off the road
            // (a rebuild), and with it the unit.
            if self.services.units[uid].leaving {
                self.services.units.remove(uid);
                continue;
            }
            let state = {
                let u = &mut self.services.units[uid];
                u.car = None;
                u.state
            };
            match state {
                UnitState::Responding => {
                    let inc = self.services.units[uid].incident.take();
                    self.park_at_station(uid);
                    if let Some(i) = inc.and_then(|i| self.services.incident_mut(i)) {
                        i.stage = Stage::Reported;
                        i.unit = None;
                        i.dispatch_at = Some(self.clock.tick);
                    }
                }
                UnitState::Transporting => {
                    let at = self.services.units[uid].hospital.and_then(|h| self.building_entrance(h));
                    if let Some(p) = at {
                        self.services.units[uid].pos = p;
                    }
                    self.deliver_patient(uid);
                }
                _ => self.park_at_station(uid),
            }
        }
    }

    // ── stations that go, calls left without a unit ──────────────────────

    /// The station still stands as a service building.
    fn station_alive(&self, station: BuildingId) -> bool {
        self.buildings.get(station).is_some_and(|b| b.service.is_some())
    }

    /// [services] Unit `uid`'s station is gone (bulldozed, or replaced). A
    /// crew on scene or carrying a patient finishes the job first (`send_home`
    /// then sees no station and sends it off); one still on its way hands
    /// the call back to the queue for another unit; the rest leave town.
    /// They used to be deleted with their car, and an incident still naming
    /// the unit crashed the tick (`invalid SlotMap key`) when its scene work
    /// finished.
    fn station_lost(&mut self, uid: UnitId) {
        let Some(u) = self.services.units.get(uid) else { return };
        if u.leaving {
            // Still parked where it could find no road out: try again.
            if u.car.is_none() {
                self.unit_leaves_town(uid);
            }
            return;
        }
        let job = u.incident.filter(|&i| self.services.incident(i).is_some_and(|x| x.active() && x.unit == Some(uid)));
        match (u.state, job) {
            (UnitState::OnScene | UnitState::Transporting, Some(_)) => {}
            (UnitState::Responding, Some(i)) => {
                self.requeue(i);
                self.unit_leaves_town(uid);
            }
            _ => self.unit_leaves_town(uid),
        }
    }

    /// [services] A unit with no station left drives off the map by the
    /// nearest exit (`unit_car_arrived` takes it out of the world there),
    /// without lights, taking no calls. A unit standing still (parked in the
    /// station, on scene, on a beat) is put on the street beside it. With
    /// no road out it waits and tries again (`station_lost`), except a unit
    /// that was parked inside its station: that goes with the building.
    fn unit_leaves_town(&mut self, uid: UnitId) {
        let Some(u) = self.services.units.get_mut(uid) else { return };
        let was_idle = u.state == UnitState::Idle && u.car.is_none();
        u.leaving = true;
        u.state = UnitState::Returning;
        u.incident = None;
        u.hospital = None;
        let (car, pos) = (u.car, u.pos);
        match car.filter(|c| self.traffic.cars.contains_key(*c)) {
            Some(car) => {
                self.traffic.cars[car].siren = false;
                if !self.redirect_out(car) {
                    self.traffic.pull_in(car, super::cars::RETIRE_RETRY_SECONDS);
                }
            }
            None => {
                self.services.units[uid].car = None;
                match self.route_out_from(pos) {
                    Some((lanes, s0, s1)) => {
                        let car = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
                        self.services.by_car.insert(car, uid);
                        self.services.units[uid].car = Some(car);
                    }
                    None if was_idle => {
                        self.services.units.remove(uid);
                    }
                    None => {}
                }
            }
        }
    }

    /// [services] Back in the dispatch queue: the unit that had the call
    /// is gone or was called off. A scene the air crew had (its work set to
    /// "until the crew is done") gets the ground crew's time again.
    pub(super) fn requeue(&mut self, id: u32) {
        let tick = self.clock.tick;
        let Some(i) = self.services.incident_mut(id) else { return };
        i.stage = Stage::Reported;
        i.unit = None;
        i.dispatched = None;
        i.arrived = None;
        i.unit_name = None;
        i.dispatch_at = Some(tick);
        if i.work_left == f32::MAX {
            i.work_left = match i.kind {
                IncidentKind::Medical => MEDICAL_WORK,
                IncidentKind::Crime => CRIME_WORK,
                IncidentKind::Crash => CRASH_WORK,
                IncidentKind::Fire => 0.0,
            };
        }
    }

    /// [services] Someone is on this job: the unit it names is out on it, or
    /// an air ambulance has it.
    fn incident_served(&self, inc: &Incident) -> bool {
        let by_unit = inc.unit.and_then(|u| self.services.units.get(u)).is_some_and(|u| u.incident == Some(inc.id) && !u.leaving);
        by_unit || self.air.drones.values().any(|d| matches!(d.job, crate::air::Job::Medic { incident, .. } if incident == inc.id))
    }

    /// [services] An active call whose unit or aircraft is gone: a patient
    /// who was being driven to hospital is delivered to the nearest free bed
    /// (or treated where they are); anything else goes back in the queue.
    fn orphaned(&mut self, id: u32) {
        let Some(inc) = self.services.incident(id).cloned() else { return };
        if inc.stage == Stage::Transporting {
            match (inc.citizen.filter(|c| self.citizens.contains_key(*c)), self.hospital_with_bed(inc.pos)) {
                (Some(p), Some(h)) => self.admit(id, p, h),
                _ => self.treat_on_site(id),
            }
            return;
        }
        self.requeue(id);
    }

    // ── repairs ────────────────────────────────────────────────────────────

    /// [services] What one of the city's own structures cost to put up
    /// (the base for its repair bill).
    fn civic_value(b: &Building) -> f32 {
        if let Some(s) = b.service {
            s.kind.build_cost(s.tier)
        } else if let Some(k) = b.plant {
            Placeable::Plant(k).cost()
        } else if let Some(r) = &b.rail {
            Placeable::Rail(r.kind).cost()
        } else if let Some(k) = b.port {
            Placeable::Dock(k).cost()
        } else if b.depot {
            Placeable::BusDepot.cost()
        } else if b.metro {
            Placeable::MetroStation.cost()
        } else {
            Placeable::CargoHub.cost()
        }
    }

    /// [services] Midnight: the city mends its own structures (stations,
    /// plants, depots, docks...), `REPAIR_PER_DAY` of condition a day at
    /// `REPAIR_SHARE` of the build price, while the treasury can pay. They
    /// do not decay with age (`daily_update`: their upkeep keeps them), so
    /// this is fire damage: a gutted station is out of service (derelict)
    /// until the repairs bring it past that line.
    fn repair_civic(&mut self) {
        let jobs: Vec<(BuildingId, f32, f32, String)> = self
            .buildings
            .iter()
            .filter(|(id, b)| Self::is_civic_structure(b) && b.construction.is_none() && b.condition < 1.0 && !self.services.is_burning(*id))
            .map(|(id, b)| {
                let amount = REPAIR_PER_DAY.min(1.0 - b.condition);
                (id, amount, amount * REPAIR_SHARE * Self::civic_value(b), b.name.clone())
            })
            .collect();
        for (id, amount, cost, name) in jobs {
            if self.economy.treasury < cost {
                continue;
            }
            self.economy.debit_item(Flow::ServiceUpkeep, cost, Item::Named(format!("repairs to {name}")));
            let b = &mut self.buildings[id];
            let was = b.is_derelict();
            b.condition = (b.condition + amount).min(1.0);
            if was != b.is_derelict() {
                self.events.push(SimEvent::BuildingChanged(id));
            }
        }
    }

    fn park_at_station(&mut self, uid: UnitId) {
        let Some(u) = self.services.units.get(uid) else { return };
        if !self.station_alive(u.station) {
            self.unit_leaves_town(uid);
            return;
        }
        let home = self.buildings.get(u.station).and_then(|b| self.lots.get(b.lot)).map(|l| l.car_point);
        let u = &mut self.services.units[uid];
        u.state = UnitState::Idle;
        u.incident = None;
        u.hospital = None;
        if let Some(p) = home {
            u.pos = p;
        }
    }

    fn step_incident(&mut self, id: u32) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident(id).cloned() else { return };
        if !inc.on_road && !self.buildings.contains_key(inc.building) {
            self.resolve(id, Outcome::Gone);
            return;
        }
        // [services] The unit or aircraft on the job is gone (its station
        // pulled down, a drone scrapped, a car lost): the call goes back in
        // the queue, and a patient on the way to hospital is delivered.
        // Such incidents used to stay active for ever, pinning the patient,
        // the crime scene or the wrecks.
        if !inc.outside && matches!(inc.stage, Stage::Dispatched | Stage::OnScene | Stage::Transporting) && !self.incident_served(&inc) {
            self.orphaned(id);
            return;
        }
        if inc.kind == IncidentKind::Fire && self.step_fire(id) {
            return;
        }
        // A patient who is no longer where they fell ill (left town, the
        // building came down) is not waiting any more.
        if inc.kind == IncidentKind::Medical && matches!(inc.stage, Stage::Unreported | Stage::Reported | Stage::Dispatched | Stage::OnScene) {
            // [driving] Hurt on the road: still out there, not taken anywhere.
            let here = inc.citizen.and_then(|c| self.citizens.get(c)).is_some_and(|c| if inc.on_road { c.inside.is_none() && !c.activity.is_traveling() } else { c.inside == Some(inc.building) });
            if !here {
                self.resolve(id, Outcome::Gone);
                return;
            }
            let c = &mut self.citizens[inc.citizen.unwrap()];
            c.next_decision_tick = c.next_decision_tick.max(tick + 30);
        }
        match inc.stage {
            Stage::Unreported => {
                if tick >= inc.report_at {
                    self.report(id);
                }
            }
            Stage::Reported => {
                let since = tick.saturating_sub(inc.reported.unwrap_or(tick));
                // (Dispatch itself runs over the whole queue: `dispatch_queue`.)
                let give_up = match inc.kind {
                    IncidentKind::Crime | IncidentKind::Crash if since > CRIME_GIVE_UP => Some(Outcome::Unsolved),
                    IncidentKind::Medical if since > MEDICAL_GIVE_UP => Some(Outcome::Recovered),
                    _ => None,
                };
                if let Some(o) = give_up {
                    let k = inc.kind.slot();
                    let city_has = self.services.units.values().any(|u| u.kind == inc.kind.unit() && self.crewed_units(u.station) > 0);
                    let free = self.free_units()[k];
                    let why = &mut self.services.stats.unanswered_why_today[k];
                    match (city_has, free) {
                        (false, _) => why.no_station += 1,
                        (true, 0) => why.all_units_busy += 1,
                        _ => why.no_route += 1,
                    }
                    self.resolve(id, o);
                }
            }
            Stage::Dispatched if inc.outside && inc.arrive_at.is_some_and(|t| tick >= t) => self.on_scene(id),
            Stage::OnScene if inc.kind != IncidentKind::Fire => {
                let done = {
                    let i = self.services.incident_mut(id).unwrap();
                    i.work_left -= 1.0;
                    i.work_left <= 0.0
                };
                if done {
                    self.finish_scene(id);
                }
            }
            _ => {}
        }
    }

    /// One tick of a fire: growth (or knock-down with an engine there),
    /// damage, spread and burning down. True when the incident ended.
    fn step_fire(&mut self, id: u32) -> bool {
        let tick = self.clock.tick;
        let (bid, fighting, intensity) = {
            let i = self.services.incident_mut(id).unwrap();
            let fighting = i.stage == Stage::OnScene;
            if fighting {
                i.intensity -= ENGINE_RATE;
            } else {
                i.intensity = (i.intensity + FIRE_GROWTH).min(1.0);
            }
            (i.building, fighting, i.intensity)
        };
        if intensity <= 0.0 {
            self.services.incident_mut(id).unwrap().intensity = 0.0;
            self.resolve(id, Outcome::Extinguished);
            return true;
        }
        // [services] The city's own structures are gutted, never razed:
        // their condition stops at the burned-down line.
        let civic = Self::is_civic_structure(&self.buildings[bid]);
        let (before, after) = {
            let b = &mut self.buildings[bid];
            let before = b.condition;
            b.condition = (b.condition - FIRE_DAMAGE * intensity * if fighting { 0.3 } else { 1.0 }).max(if civic { BURNED_DOWN } else { 0.0 });
            (before, b.condition)
        };
        if (before * 10.0).floor() != (after * 10.0).floor() {
            self.events.push(SimEvent::BuildingChanged(bid));
        }
        let bname = self.buildings[bid].name.clone();
        if let Some(i) = self.services.incident_mut(id) {
            match i.damage.iter_mut().find(|d| d.building == bid) {
                Some(d) => d.lost += before - after,
                None => i.damage.push(Damage { building: bid, name: bname, lost: before - after, destroyed: false }),
            }
        }
        if after <= BURNED_DOWN {
            if civic {
                self.gutted(id, bid);
            } else {
                self.burn_down(id, bid);
            }
            return true;
        }
        if intensity > SPREAD_INTENSITY && tick % SPREAD_EVERY == 0 {
            let here = self.building_lot(bid).map(|l| (l.center, l.polygon.clone()));
            if let Some((here, poly)) = here {
                // Neighbours: lots that touch this one (a shared corner within
                // SPREAD_RADIUS of reach) or stand that close centre to centre.
                let touches = |l: &Lot| l.center.distance(here) <= SPREAD_RADIUS || (l.center.distance(here) < 150.0 && l.polygon.iter().any(|p| poly.iter().any(|q| q.distance(*p) < SPREAD_GAP)));
                let near: Vec<BuildingId> = self.buildings.iter().filter(|(nid, nb)| *nid != bid && self.lots.get(nb.lot).is_some_and(touches)).map(|(nid, _)| nid).collect();
                for nb in near {
                    if self.srng().random::<f32>() < SPREAD_CHANCE * intensity {
                        if let Some(new) = self.start_fire(nb) {
                            // The neighbours see it coming: a quick call.
                            if let Some(i) = self.services.incident_mut(new) {
                                i.report_at = tick + 1;
                            }
                            self.events.push(SimEvent::FireSpread(nb));
                        }
                    }
                }
            }
        }
        false
    }

    /// [services] A civic structure (station, plant, depot, dock...) that
    /// burned out: the fire is over, the building stands gutted and out of
    /// service (derelict: no crews, no coverage) until the city's repairs
    /// (`repair_civic`, paid daily) bring it back. Burning them down removed
    /// the player's building outright and orphaned its units' calls.
    fn gutted(&mut self, id: u32, bid: BuildingId) {
        let tick = self.clock.tick;
        let Some(b) = self.buildings.get(bid) else { return };
        let name = b.name.clone();
        let people: Vec<CitizenId> = b.workers.clone();
        for p in people {
            if let Some(c) = self.citizens.get_mut(p) {
                c.remember(tick, format!("{name} burned out. Shut until the city fixes it."));
            }
        }
        self.events.push(SimEvent::BuildingChanged(bid));
        self.resolve(id, Outcome::Gutted);
    }

    fn burn_down(&mut self, id: u32, bid: BuildingId) {
        let Some(b) = self.buildings.get(bid) else { return };
        let (lot, name) = (b.lot, b.name.clone());
        let center = self.lots.get(lot).map_or(Vec2::ZERO, |l| l.center);
        self.events.push(SimEvent::BurnedDown(bid, lot));
        self.demolish_with(bid, None, Some(&format!("{name} burned down. Lost everything.")));
        if let Some(l) = self.lots.get_mut(lot) {
            if l.building == Some(bid) {
                l.building = None;
            }
        }
        let tick = self.clock.tick;
        self.services.rubble.push((lot, center, tick + RUBBLE_TICKS));
        self.services.stats.burned_down_total += 1;
        if let Some(i) = self.services.incident_mut(id) {
            for d in i.damage.iter_mut().filter(|d| d.building == bid) {
                d.destroyed = true;
            }
        }
        self.resolve(id, Outcome::BurnedDown);
        // Anything else going on in there is over too.
        let others: Vec<u32> = self.services.active().filter(|i| i.building == bid).map(|i| i.id).collect();
        for o in others {
            self.resolve(o, Outcome::Gone);
        }
    }

    // ── the 911 call and the dispatcher ────────────────────────────────────

    /// Who answers 911 right now and how long the job takes to go out: a
    /// dispatcher at the desk, one on call, or the regional line.
    pub fn dispatcher(&self) -> (Option<CitizenId>, u64) {
        let centres: Vec<(BuildingId, &Building)> = self.buildings.iter().filter(|(_, b)| b.service.is_some_and(|s| s.kind == ServiceKind::Dispatch) && !b.is_derelict() && b.construction.is_none()).collect();
        let mut on_call = None;
        for (id, b) in &centres {
            for &w in &b.workers {
                if self.citizens.get(w).is_some_and(|c| c.inside == Some(*id)) {
                    let fast = if b.tier == Tier::High { DISPATCH_ON_SHIFT - 1 } else { DISPATCH_ON_SHIFT };
                    return (Some(w), fast);
                }
                on_call = on_call.or(Some(w));
            }
        }
        match on_call {
            Some(w) => (Some(w), DISPATCH_ON_CALL),
            None => (None, DISPATCH_REGIONAL),
        }
    }

    fn report(&mut self, id: u32) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident(id).cloned() else { return };
        let bname = self.buildings.get(inc.building).map(|b| b.name.clone()).unwrap_or_default();
        // The caller: someone inside, else someone who lives or works there.
        let inside: Vec<CitizenId> = self.citizens.iter().filter(|(cid, c)| c.inside == Some(inc.building) && Some(*cid) != inc.citizen && Some(*cid) != inc.suspect).map(|(cid, _)| cid).collect();
        let caller = if inc.on_road {
            // [driving] Whoever saw it from the street.
            inc.caller
        } else {
            inside.choose(self.srng()).copied().or_else(|| self.buildings.get(inc.building).and_then(|b| b.residents.first().or(b.workers.first()).copied())).or(if inc.kind == IncidentKind::Crime { inc.citizen } else { None })
        };
        let (dispatcher, delay) = self.dispatcher();
        {
            let i = self.services.incident_mut(id).unwrap();
            i.stage = Stage::Reported;
            i.reported = Some(tick);
            i.caller = caller;
            i.dispatcher = dispatcher;
            i.dispatch_at = Some(tick + delay);
        }
        let named: Vec<(CitizenId, String, &str)> = [(caller, "caller"), (dispatcher, "dispatcher")].into_iter().filter_map(|(c, role)| c.and_then(|c| self.citizens.get(c).map(|x| (c, x.name.clone(), role)))).collect();
        if let Some(i) = self.services.incident_mut(id) {
            for (c, n, role) in named {
                i.involve(c, &n, role);
            }
        }
        let what = match inc.kind {
            IncidentKind::Fire => format!("smoke pouring out of {bname}"),
            IncidentKind::Crime => format!("a {} at {bname}", inc.crime.map_or("robbery", |c| c.label())),
            IncidentKind::Medical if inc.on_road => format!("{} hurt in a crash by {bname}", inc.citizen.and_then(|c| self.citizens.get(c)).map_or("someone".to_string(), |c| c.name.clone())),
            IncidentKind::Medical => format!("{} collapsed at {bname}", inc.citizen.and_then(|c| self.citizens.get(c)).map_or("someone".to_string(), |c| c.name.clone())),
            IncidentKind::Crash => format!("a bad crash by {bname}"),
        };
        if let Some(c) = caller.and_then(|c| self.citizens.get_mut(c)) {
            c.remember(tick, format!("Called 911: {what}."));
            c.thought = "I called 911.".to_string();
        }
        self.events.push(SimEvent::EmergencyCall(id));
    }

    /// Send the nearest free unit of the right kind: one in a station with a
    /// crew to spare, on patrol or standby, or on its way home, each from
    /// where it is. With no crewed station of the kind in the city, a
    /// dispatcher calls the next town. True when help is on the way.
    fn try_dispatch(&mut self, id: u32) -> bool {
        let Some(inc) = self.services.incident(id).cloned() else { return false };
        // No crewed station of the kind in the whole city: a dispatcher can
        // ask a neighbouring town, which sends a unit slowly and bills us.
        if !self.city_has(inc.kind) {
            if inc.dispatcher.is_some() {
                self.send_from_out_of_town(id, OUT_OF_TOWN_TRAVEL, false);
                return true;
            }
            return false;
        }
        let Some(to) = self.incident_access(id) else { return false };
        let uk = inc.kind.unit();
        let out = self.units_out();
        let mut free: Vec<(f32, UnitId)> = self
            .services
            .units
            .iter()
            .filter(|(_, u)| u.kind == uk && self.unit_available(u, &out))
            .map(|(uid, u)| (u.car.and_then(|c| self.traffic.cars.get(c)).map_or(u.pos, |c| c.pos).distance(inc.pos), uid))
            .collect();
        free.sort_by(|a, b| a.0.total_cmp(&b.0));
        free.into_iter().any(|(_, uid)| self.send_unit(uid, id, to))
    }

    /// The quickest way for lights and sirens from `from` to `to`: a unit
    /// may turn round in the street and pull up on either side, so the
    /// route is planned from and to both sides of the road and the shortest
    /// taken. (From the lane it happened
    /// to face, a car parked a block from a call drove round the block, and
    /// sometimes round two.)
    fn siren_route(&self, from: (SegmentId, Vec2), to: (SegmentId, Vec2)) -> Option<(Vec<crate::traffic::LaneId>, f32, f32)> {
        let net = &self.traffic.net;
        let length = |r: &(Vec<crate::traffic::LaneId>, f32, f32)| r.0.iter().map(|l| net.lanes[*l as usize].length).sum::<f32>() - r.1 - net.lanes[*r.0.last().unwrap() as usize].length + r.2;
        // A point and its mirror across the street's centre line.
        let sides = |(seg, p): (SegmentId, Vec2)| -> Vec<Vec2> {
            match self.roads.nearest_street(p).filter(|(s, _, q)| *s == seg && q.distance(p) > 0.5) {
                Some((_, _, q)) => vec![p, q * 2.0 - p],
                None => vec![p],
            }
        };
        let ends = sides(to);
        sides(from)
            .into_iter()
            .flat_map(|a| ends.iter().map(move |b| (a, *b)))
            .filter_map(|(a, b)| net.find_route(from.0, a, to.0, b))
            .filter(|r| !r.0.is_empty())
            .min_by(|a, b| length(a).total_cmp(&length(b)))
    }

    /// Help from the next town: billed, and there in `travel` minutes.
    /// `mutual` for mutual aid (the city has units, all busy).
    fn send_from_out_of_town(&mut self, id: u32, travel: u64, mutual: bool) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident(id).cloned() else { return };
        let uk = inc.kind.unit();
        let fee = OUT_OF_TOWN_FEE[inc.kind.slot()];
        self.economy.debit_item(Flow::ServiceUpkeep, fee, Item::Named(format!("{} from the next town", uk.label())));
        self.services.stats.out_of_town_calls_today += 1;
        self.services.stats.out_of_town_fees_today += fee;
        if let Some(d) = inc.dispatcher.and_then(|d| self.citizens.get_mut(d)) {
            d.remember(tick, if mutual { format!("Every {} we have was out. Asked the next town to cover {}.", uk.label(), inc.place) } else { format!("No {} of ours to send to {}. Called the next town over.", uk.label(), inc.place) });
        }
        self.services.route_failed.remove(&id);
        let i = self.services.incident_mut(id).unwrap();
        i.stage = Stage::Dispatched;
        i.dispatched = Some(tick);
        i.outside = true;
        i.fee = fee;
        i.arrive_at = Some(tick + travel);
        i.unit_name = Some(if mutual { format!("{} from the next town (mutual aid)", uk.label()) } else { format!("{} from out of town", uk.label()) });
        self.events.push(SimEvent::UnitDispatched(id));
    }

    /// Put `uid` on the road to incident `id` with lights and sirens, from
    /// its station or from wherever it is driving or parked. False if no
    /// road leads there.
    fn send_unit(&mut self, uid: UnitId, id: u32, to: (SegmentId, Vec2)) -> bool {
        let tick = self.clock.tick;
        let Some(u) = self.services.units.get(uid) else { return false };
        let (station, state) = (u.station, u.state);
        let here = u.car.and_then(|c| self.traffic.cars.get(c)).map_or(u.pos, |c| c.pos);
        let from = if state == UnitState::Idle { self.building_lot(station).and_then(|l| self.lot_access(l)) } else { self.roads.nearest_street(here).map(|(s, _, _)| (s, here)) };
        let Some(from) = from else { return false };
        let Some((lanes, s0, s1)) = self.siren_route(from, to) else { return false };
        self.pull_off_road(uid);
        let car = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
        self.traffic.cars[car].siren = true;
        self.services.by_car.insert(car, uid);
        let (call, number) = {
            let u = &mut self.services.units[uid];
            u.state = UnitState::Responding;
            u.car = Some(car);
            u.incident = Some(id);
            u.hospital = None;
            (u.kind.call_sign(), u.number)
        };
        let sname = self.buildings.get(station).map(|b| b.name.clone()).unwrap_or_default();
        let (kind, bid, dispatcher) = {
            let i = self.services.incident_mut(id).unwrap();
            i.stage = Stage::Dispatched;
            i.dispatched = Some(tick);
            i.unit = Some(uid);
            i.unit_name = Some(format!("{call} {number}, {sname}"));
            (i.kind, i.building, i.dispatcher)
        };
        let bname = self.buildings.get(bid).map(|b| b.name.clone()).unwrap_or_default();
        if let Some(d) = dispatcher.and_then(|d| self.citizens.get_mut(d)) {
            d.remember(tick, format!("Took a 911 call: {} at {bname}. Sent {call} {number} from {sname}.", kind.label()));
        }
        self.events.push(SimEvent::UnitDispatched(id));
        true
    }

    // ── units on the road ──────────────────────────────────────────────────

    /// An emergency vehicle reached the end of its route (called from
    /// `step_traffic`).
    pub(super) fn unit_car_arrived(&mut self, car_id: CarId) {
        // [services] Leaving town: gone at the map edge, else (it held for
        // want of a road out) it tries again from here.
        if let Some(uid) = self.services.by_car.get(&car_id).copied().filter(|u| self.services.units.get(*u).is_some_and(|u| u.leaving)) {
            if self.at_map_edge(car_id) {
                self.services.by_car.remove(&car_id);
                self.traffic.remove_car(car_id);
                self.services.units.remove(uid);
            } else if !self.redirect_out(car_id) {
                self.traffic.pull_in(car_id, super::cars::RETIRE_RETRY_SECONDS);
            }
            return;
        }
        // [emergency] In pursuit: it stays on the road after its quarry.
        if self.pursuit_arrived(car_id) {
            return;
        }
        let Some(uid) = self.services.by_car.remove(&car_id) else { return };
        let Some(car) = self.traffic.remove_car(car_id) else { return };
        let Some(u) = self.services.units.get_mut(uid) else { return };
        u.car = None;
        u.pos = car.pos;
        u.heading = car.heading;
        let state = u.state;
        match state {
            UnitState::Responding => {
                let inc = u.incident;
                let live = inc.and_then(|i| self.services.incident(i)).is_some_and(|i| i.stage == Stage::Dispatched);
                if !live {
                    self.send_home(uid);
                    return;
                }
                self.services.units[uid].state = UnitState::OnScene;
                self.on_scene(inc.unwrap());
            }
            UnitState::Transporting => self.deliver_patient(uid),
            UnitState::Returning => self.park_at_station(uid),
            UnitState::Patrol => {
                // On the beat: park a while.
                let stay = if self.services.units[uid].kind == UnitKind::PoliceCar { PATROL_STAY } else { STANDBY_STAY };
                let until = self.clock.tick + self.srng().random_range(stay.0..=stay.1);
                self.services.units[uid].patrol_until = until;
            }
            _ => self.send_home(uid),
        }
    }

    /// A unit (city or out of town) got there: the clock on the response
    /// stops and the people involved see it arrive.
    pub(super) fn on_scene(&mut self, id: u32) {
        let tick = self.clock.tick;
        let (kind, bid, response) = {
            let i = self.services.incident_mut(id).unwrap();
            i.stage = Stage::OnScene;
            i.arrived = Some(tick);
            (i.kind, i.building, i.response_ticks())
        };
        if let Some(r) = response {
            self.services.stats.response_ticks_today += r;
            self.services.stats.responses_today += 1;
            self.services.stats.response_min_today[kind.slot()].push(r.min(u16::MAX as u64) as u16);
        }
        let bname = self.buildings.get(bid).map(|b| b.name.clone()).unwrap_or_default();
        let note = match kind {
            IncidentKind::Fire => format!("The fire brigade arrived at {bname}, sirens going."),
            IncidentKind::Crime => format!("The police showed up at {bname}."),
            IncidentKind::Medical => format!("Paramedics came for me at {bname}."),
            IncidentKind::Crash => format!("The police came to the crash by {bname}."),
        };
        let who: Vec<CitizenId> = match kind {
            IncidentKind::Medical => self.services.incident(id).and_then(|i| i.citizen).into_iter().collect(),
            IncidentKind::Crash => self.services.incident(id).map(|i| i.people.iter().map(|p| p.citizen).collect()).unwrap_or_default(),
            _ => self.buildings.get(bid).map(|b| b.residents.iter().chain(b.workers.iter()).copied().collect()).unwrap_or_default(),
        };
        // [concerns] Help that took this long is a grievance.
        let slow = response.filter(|r| *r >= crate::concerns::SLOW_RESPONSE_TICKS).map(|r| {
            let unit = match kind {
                IncidentKind::Fire => "the fire brigade",
                IncidentKind::Medical => "the ambulance",
                _ => "the police",
            };
            format!("Took {r} minutes for {unit} to get to {bname}.")
        });
        for (i, cid) in who.into_iter().enumerate() {
            if let Some(c) = self.citizens.get_mut(cid) {
                c.remember(tick, note.clone());
                if let Some(text) = slow.as_ref().filter(|_| i < 8) {
                    self.concerns.said(tick, cid, c, crate::concerns::Concern::Services, 0.5, text.clone());
                }
            }
        }
        self.events.push(SimEvent::UnitOnScene(id));
    }

    /// Drive a car for `uid` from where it stands to `to`. False if no road.
    pub(super) fn drive_unit(&mut self, uid: UnitId, to: (SegmentId, Vec2), siren: bool) -> bool {
        let Some(from) = self.services.units.get(uid).map(|u| u.pos) else { return false };
        let Some((seg, _, q)) = self.roads.nearest_street(from) else { return false };
        let Some((lanes, s0, s1)) = self.traffic.net.find_route(seg, q, to.0, to.1) else { return false };
        let car = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
        self.traffic.cars[car].siren = siren;
        self.services.by_car.insert(car, uid);
        self.services.units[uid].car = Some(car);
        true
    }

    /// Back to the station, or teleported home when no road leads there.
    /// Police cars and ambulances with a crew go straight back on patrol or
    /// standby instead.
    pub(super) fn send_home(&mut self, uid: UnitId) {
        let Some(u) = self.services.units.get(uid) else { return };
        if !self.station_alive(u.station) {
            self.unit_leaves_town(uid);
            return;
        }
        if matches!(u.kind, UnitKind::PoliceCar | UnitKind::Ambulance) && u.car.is_none() {
            let out = self.units_out();
            if out.get(&u.station).copied().unwrap_or(0) <= self.crewed_units(u.station) && self.start_patrol(uid) {
                return;
            }
        }
        let Some(u) = self.services.units.get_mut(uid) else { return };
        u.incident = None;
        u.hospital = None;
        let station = u.station;
        let to = self.building_lot(station).and_then(|l| self.lot_access(l));
        match to {
            Some(to) if self.drive_unit(uid, to, false) => self.services.units[uid].state = UnitState::Returning,
            _ => self.park_at_station(uid),
        }
    }

    /// Work on scene is done (crimes and patients; fires end when out).
    fn finish_scene(&mut self, id: u32) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident(id).cloned() else { return };
        match inc.kind {
            IncidentKind::Crime => {
                // [emergency] A suspect who can get to a car may run for it.
                if self.try_chase(id) {
                    return;
                }
                let quick = inc.arrived.unwrap_or(tick).saturating_sub(inc.started) <= QUICK_RESPONSE;
                let caught = inc.suspect.is_some() && self.srng().random::<f32>() < if quick { ARREST_QUICK } else { ARREST_SLOW };
                let bname = self.buildings.get(inc.building).map(|b| b.name.clone()).unwrap_or_default();
                if caught {
                    if let Some(s) = inc.suspect.and_then(|s| self.citizens.get_mut(s)) {
                        s.remember(tick, format!("Got picked up by the cops over the job at {bname}."));
                        s.mood = (s.mood - 0.2).max(0.0);
                        s.thought = "Busted.".to_string();
                    }
                    if let Some(v) = inc.citizen.and_then(|v| self.citizens.get_mut(v)) {
                        v.remember(tick, "The police caught whoever robbed me.");
                    }
                    self.services.stats.arrests_today += 1;
                    if let Some(s) = inc.suspect {
                        self.take_into_custody(id, s);
                    }
                    self.resolve(id, Outcome::Arrest);
                } else {
                    self.resolve(id, Outcome::Escaped);
                }
            }
            IncidentKind::Medical => {
                let hospital = self.hospital_with_bed(inc.pos);
                let uid = inc.unit;
                match (hospital, uid) {
                    (Some(h), Some(uid)) => {
                        let to = self.building_lot(h).and_then(|l| self.lot_access(l));
                        if to.is_some_and(|to| self.drive_unit(uid, to, true)) {
                            let u = &mut self.services.units[uid];
                            u.state = UnitState::Transporting;
                            u.hospital = Some(h);
                            self.services.incident_mut(id).unwrap().stage = Stage::Transporting;
                        } else {
                            self.treat_on_site(id);
                        }
                    }
                    (Some(h), None) if inc.outside => {
                        if let Some(p) = inc.citizen {
                            self.admit(id, p, h);
                        }
                    }
                    _ => self.treat_on_site(id),
                }
            }
            IncidentKind::Fire => {}
            IncidentKind::Crash => {
                // A drunk or a red-light runner who is not in an ambulance
                // may be taken in; the wrecks are towed (`World::step_driving`).
                let suspect = inc.suspect.filter(|&s| self.citizens.contains_key(s) && !self.is_patient(s) && !self.services.in_custody(s));
                if suspect.is_some() && self.srng().random::<f32>() < DUI_ARREST {
                    let s = suspect.unwrap();
                    let c = &mut self.citizens[s];
                    c.remember(tick, "Arrested at the scene of my crash.");
                    c.mood = (c.mood - 0.2).max(0.0);
                    c.thought = "Should never have got behind the wheel.".to_string();
                    self.services.stats.arrests_today += 1;
                    self.take_into_custody(id, s);
                    self.resolve(id, Outcome::Arrest);
                } else {
                    self.resolve(id, Outcome::Cleared);
                }
            }
        }
    }

    pub(super) fn treat_on_site(&mut self, id: u32) {
        let tick = self.clock.tick;
        if let Some(c) = self.services.incident(id).and_then(|i| i.citizen).and_then(|c| self.citizens.get_mut(c)) {
            c.remember(tick, "Patched up by paramedics where I fell. No bed free anywhere.");
            c.next_decision_tick = tick + 60;
        }
        if let Some(cid) = self.services.incident(id).and_then(|i| i.citizen) {
            if let Some(c) = self.citizens.get(cid) {
                self.concerns.heard(cid, c, crate::concerns::Concern::Services, 0.5);
            }
        }
        self.resolve(id, Outcome::TreatedOnSite);
    }

    /// The nearest hospital with a free bed.
    fn hospital_with_bed(&self, p: Vec2) -> Option<BuildingId> {
        self.buildings
            .iter()
            .filter_map(|(id, b)| b.service.filter(|s| s.kind == ServiceKind::Health).map(|s| (id, b, s)))
            .filter(|(_, b, _)| !b.is_derelict())
            .filter(|(id, _, s)| (self.services.admissions.iter().filter(|a| a.hospital == *id).count() as u32) < s.kind.capacity(s.tier))
            .filter_map(|(id, b, _)| self.lots.get(b.lot).map(|l| (id, l.center.distance(p))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// The ambulance is at the hospital door: the patient goes to a bed.
    fn deliver_patient(&mut self, uid: UnitId) {
        let Some((inc, hospital)) = self.services.units.get(uid).map(|u| (u.incident, u.hospital)) else { return };
        let patient = inc.and_then(|i| self.services.incident(i)).and_then(|i| i.citizen);
        match (inc, hospital, patient) {
            (Some(id), Some(h), Some(p)) if self.buildings.contains_key(h) && self.citizens.contains_key(p) => self.admit(id, p, h),
            (Some(id), _, _) => self.resolve(id, Outcome::Gone),
            _ => {}
        }
        self.send_home(uid);
    }

    /// A patient into a hospital bed; the household is billed.
    pub(super) fn admit(&mut self, id: u32, p: CitizenId, h: BuildingId) {
        let tick = self.clock.tick;
        {
            {
                self.leave_building(p);
                let entrance = self.building_entrance(h).unwrap_or(Vec2::ZERO);
                let hours = self.srng().random_range(STAY_HOURS.0..=STAY_HOURS.1);
                let until = tick + hours * 60;
                let hname = self.buildings[h].name.clone();
                self.buildings[h].visitors += 1;
                let c = &mut self.citizens[p];
                c.inside = Some(h);
                c.activity = Activity::Visiting(h);
                c.pos = entrance;
                c.prev_pos = entrance;
                c.next_decision_tick = until;
                c.thought = "Hooked up to a drip. At least it's warm.".to_string();
                c.remember(tick, format!("The ambulance took me to {hname}."));
                self.services.admissions.push(Admission { citizen: p, hospital: h, until });
                // The household pays what it can; the city covers the rest.
                let paid = match self.citizens[p].household.and_then(|hh| self.households.get_mut(hh)) {
                    Some(hh) => {
                        let pay = hh.money.clamp(0.0, HOSPITAL_BILL);
                        hh.money -= pay;
                        pay
                    }
                    None => 0.0,
                };
                self.economy.credit_item(Flow::ServiceUpkeep, paid, Item::Named("hospital bills paid by patients".into()));
                self.services.stats.bills_paid_today += paid;
                self.services.stats.bills_covered_today += HOSPITAL_BILL - paid;
                if let Some(i) = self.services.incident_mut(id) {
                    i.taken(p, &hname);
                }
                self.resolve(id, Outcome::Admitted);
            }
        }
    }

    /// An arrested suspect goes to a jail with room for a sentence, or is
    /// held at the nearest police station for a few hours and let go.
    pub(super) fn take_into_custody(&mut self, id: u32, s: CitizenId) {
        let tick = self.clock.tick;
        if !self.citizens.contains_key(s) {
            return;
        }
        let at = self.services.incident(id).map_or(Vec2::ZERO, |i| i.pos);
        let occupancy = self.services.jail_occupancy();
        let nearest = |w: &World, kind: ServiceKind, room: bool| -> Option<BuildingId> {
            w.buildings
                .iter()
                .filter_map(|(bid, b)| b.service.filter(|x| x.kind == kind).map(|x| (bid, b, x)))
                .filter(|(_, b, _)| !b.is_derelict() && b.construction.is_none())
                .filter(|(bid, _, x)| !room || occupancy.iter().find(|o| o.0 == *bid).map_or(0, |o| o.1) < x.kind.capacity(x.tier))
                .filter_map(|(bid, b, _)| w.lots.get(b.lot).map(|l| (bid, l.center.distance(at))))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(bid, _)| bid)
        };
        let (place, jailed, until) = match nearest(self, ServiceKind::Jail, true) {
            Some(j) => {
                let days = self.srng().random_range(SENTENCE_DAYS.0..=SENTENCE_DAYS.1);
                (Some(j), true, tick + days * 1440)
            }
            None => (nearest(self, ServiceKind::Police, false), false, tick + HELD_HOURS * 60),
        };
        match self.services.record.iter_mut().find(|r| r.0 == s) {
            Some(r) => r.1 += 1,
            None => self.services.record.push((s, 1)),
        }
        self.events.push(SimEvent::Arrested(id, s));
        let Some(place) = place else {
            // No station to hold them at: a caution on the spot.
            if let Some(i) = self.services.incident_mut(id) {
                i.taken(s, "let go with a caution");
            }
            return;
        };
        self.leave_building(s);
        let entrance = self.building_entrance(place).unwrap_or(at);
        let pname = self.buildings[place].name.clone();
        self.buildings[place].visitors += 1;
        let c = &mut self.citizens[s];
        c.inside = Some(place);
        c.activity = Activity::Visiting(place);
        c.pos = entrance;
        c.prev_pos = entrance;
        c.next_decision_tick = until;
        c.thought = if jailed { "Four walls and a bunk.".to_string() } else { "They'll have to let me go soon.".to_string() };
        c.remember(tick, if jailed { format!("Locked up in {pname}.") } else { format!("Held at {pname}. No jail to send me to.") });
        self.services.custody.push(Custody { citizen: s, at: place, jailed, until });
        if let Some(i) = self.services.incident_mut(id) {
            i.taken(s, &if jailed { format!("{pname} (jail)") } else { format!("{pname} (held)") });
        }
    }

    /// Custody runs out, or the person is somewhere else: they walk free.
    /// Prisoners are fed.
    fn step_custody(&mut self) {
        let tick = self.clock.tick;
        let mut out = Vec::new();
        for (k, c) in self.services.custody.iter().enumerate() {
            let here = self.citizens.get(c.citizen).is_some_and(|x| x.inside == Some(c.at));
            if !here || tick >= c.until {
                out.push(k);
            }
        }
        for k in out.into_iter().rev() {
            let c = self.services.custody.remove(k);
            if !c.jailed {
                self.services.stats.released_early_total += 1;
            }
            if let Some(x) = self.citizens.get_mut(c.citizen) {
                if x.inside == Some(c.at) {
                    x.remember(tick, if c.jailed { "Did my time. Out." } else { "Let go. No jail in this town." });
                    x.next_decision_tick = tick + 1;
                }
            }
            self.events.push(SimEvent::Released(c.citizen));
        }
        for c in &self.services.custody {
            if let Some(x) = self.citizens.get_mut(c.citizen) {
                x.satiety = x.satiety.max(0.6);
                x.next_decision_tick = x.next_decision_tick.max(c.until);
            }
        }
        let s = &mut self.services.stats;
        s.jailed = self.services.custody.iter().filter(|c| c.jailed).count() as u32;
        s.held_at_stations = self.services.custody.iter().filter(|c| !c.jailed).count() as u32;
    }

    /// Close an incident. The unit on scene, if any, heads home.
    pub(super) fn resolve(&mut self, id: u32, outcome: Outcome) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident_mut(id) else { return };
        if !inc.active() {
            return;
        }
        inc.stage = Stage::Resolved;
        inc.resolved = Some(tick);
        inc.outcome = Some(outcome);
        let (kind, unit, citizen, bid, answered) = (inc.kind, inc.unit, inc.citizen, inc.building, inc.arrived.is_some());
        if let Some(n) = self.services.stats.resolved_today.get_mut(kind.index()) {
            *n += 1;
        }
        if !answered && !matches!(outcome, Outcome::Gone) {
            self.services.stats.unanswered_today += 1;
        }
        match outcome {
            Outcome::Unsolved => {
                if let Some(c) = citizen.and_then(|c| self.citizens.get_mut(c)) {
                    c.remember(tick, "Nobody came about the robbery. Nobody ever does.");
                    self.concerns.heard(citizen.unwrap_or_default(), c, crate::concerns::Concern::Services, 0.6);
                    c.mood = (c.mood - 0.08).max(0.0);
                }
            }
            Outcome::Recovered => {
                if let Some(c) = citizen.and_then(|c| self.citizens.get_mut(c)) {
                    c.remember(tick, "Waited hours for an ambulance that never came. Pulled through on my own.");
                    self.concerns.heard(citizen.unwrap_or_default(), c, crate::concerns::Concern::Services, 0.8);
                    c.mood = (c.mood - 0.15).max(0.0);
                    c.next_decision_tick = tick + 30;
                }
            }
            Outcome::Gone => {
                if let Some(c) = citizen.and_then(|c| self.citizens.get_mut(c)) {
                    c.next_decision_tick = c.next_decision_tick.min(tick + 30);
                }
            }
            Outcome::Extinguished => {
                let name = self.buildings.get(bid).map(|b| b.name.clone()).unwrap_or_default();
                let people: Vec<CitizenId> = self.buildings.get(bid).map(|b| b.residents.iter().chain(b.workers.iter()).copied().collect()).unwrap_or_default();
                for p in people {
                    if let Some(c) = self.citizens.get_mut(p) {
                        c.remember(tick, format!("They put the fire out at {name}."));
                    }
                }
            }
            _ => {}
        }
        if let Some(uid) = unit {
            let u = self.services.units.get(uid).filter(|u| u.incident == Some(id)).map(|u| u.state);
            if u == Some(UnitState::OnScene) {
                self.send_home(uid);
            } else if u == Some(UnitState::Responding) {
                // Called off on the way: free at once, from where it is.
                self.pull_off_road(uid);
                self.send_home(uid);
            }
        }
        self.services.route_failed.remove(&id);
        self.events.push(SimEvent::IncidentResolved(id));
    }

    // ── stadium nights and the daily books ─────────────────────────────────

    /// School-age children at home or about go to the nearest school whose
    /// district covers their home and that has places left.
    fn send_children_to_school(&mut self) {
        let tick = self.clock.tick;
        let mut schools: Vec<(BuildingId, Vec2, u32, LandUse)> = self
            .buildings
            .iter()
            .filter_map(|(id, b)| b.service.filter(|s| s.kind == ServiceKind::School).map(|s| (id, b, s)))
            .filter(|(_, b, _)| !b.is_derelict() && b.construction.is_none())
            .filter_map(|(id, b, s)| self.lots.get(b.lot).map(|l| (id, l.center, s.kind.capacity(s.tier), b.land_use)))
            .collect();
        if schools.is_empty() {
            return;
        }
        let kids: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.goes_to_school() && matches!(c.activity, Activity::AtHome | Activity::Idle) && c.home.is_some()).map(|(id, _)| id).collect();
        for cid in kids {
            if self.is_patient(cid) {
                continue;
            }
            let Some(home_lot) = self.citizens[cid].home.and_then(|h| self.buildings.get(h)).map(|b| b.lot) else { continue };
            if self.services.coverage_of(home_lot, ServiceKind::School) <= 0.0 {
                continue;
            }
            let at = self.lots.get(home_lot).map_or(Vec2::ZERO, |l| l.center);
            let Some(k) = (0..schools.len()).filter(|&k| schools[k].2 > 0).min_by(|&a, &b| schools[a].1.distance(at).total_cmp(&schools[b].1.distance(at))) else { break };
            schools[k].2 -= 1;
            let (sid, lu) = (schools[k].0, schools[k].3);
            let name = self.buildings[sid].name.clone();
            let c = &mut self.citizens[cid];
            c.asleep = false;
            c.remember(tick, format!("Off to {name} for the day."));
            self.services.stats.pupils_today += 1;
            // [passengers] Driven there by a parent now and then.
            if !self.school_run(cid, sid, Purpose::Visit(lu)) {
                self.travel_to(cid, Some(sid), Purpose::Visit(lu));
            }
        }
    }

    fn start_stadium_events(&mut self) {
        let tick = self.clock.tick;
        let stadiums: Vec<BuildingId> = self.services.events_today.iter().copied().filter(|b| self.buildings.get(*b).is_some_and(|b| !b.is_derelict())).collect();
        for sid in stadiums {
            let (seats, lu, name, at) = {
                let b = &self.buildings[sid];
                let s = b.service.unwrap();
                (s.kind.capacity(s.tier), b.land_use, b.name.clone(), self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.center))
            };
            let mut fans: Vec<CitizenId> = self
                .citizens
                .iter()
                .filter(|(_, c)| matches!(c.activity, Activity::AtHome | Activity::Idle) && c.savings - TICKET_PRICE > OVERDRAFT && c.pos.distance(at) <= EVENT_REACH)
                .map(|(id, _)| id)
                .filter(|id| !self.is_patient(*id))
                .collect();
            fans.shuffle(self.srng());
            let take = ((seats as f32 * self.srng().random_range(0.5..0.95)) as usize).min(fans.len());
            for &cid in &fans[..take] {
                let c = &mut self.citizens[cid];
                c.savings -= TICKET_PRICE;
                c.remember(tick, format!("Got a ticket for tonight's game at {name}."));
                c.thought = "Game night!".to_string();
                self.economy.credit_item(Flow::Tickets, TICKET_PRICE, Item::Building(sid));
                self.services.stats.tickets_today += TICKET_PRICE;
                self.travel_to(cid, Some(sid), Purpose::Visit(lu));
            }
            self.services.stats.stadium_crowd_today += take as u32;
            self.events.push(SimEvent::StadiumEvent(sid));
        }
    }

    /// Midnight: upkeep, the schooling wage bonus, tonight's games, and the
    /// day's counters roll over. Runs before `daily_update` closes the books.
    pub(super) fn daily_services(&mut self) {
        self.repair_civic();
        let sites: Vec<(BuildingId, f32)> = self.buildings.iter().filter_map(|(id, b)| b.service.map(|s| (id, s.kind.upkeep_per_day(s.tier)))).collect();
        let upkeep: f32 = sites.iter().map(|(_, x)| x).sum();
        for (id, x) in sites {
            self.economy.debit_item(Flow::ServiceUpkeep, x, Item::Building(id));
        }
        // Workers living near a school earn more: night classes, a trade.
        let mut bonus = 0.0;
        let cids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in cids {
            let c = &self.citizens[cid];
            let (Some(work), Some(home)) = (c.work, c.home) else { continue };
            let (Some(wb), Some(hb)) = (self.buildings.get(work), self.buildings.get(home)) else { continue };
            let cov = self.services.coverage_of(hb.lot, ServiceKind::School);
            let base = wb.wage() * Self::skill_of(c.seed);
            let income = base * (1.0 + EDUCATION_BONUS * cov);
            bonus += income - base;
            self.citizens[cid].income = income;
        }
        let stadiums: Vec<(BuildingId, Tier)> = self.buildings.iter().filter_map(|(id, b)| b.service.filter(|s| s.kind == ServiceKind::Stadium).map(|s| (id, s.tier))).collect();
        let mut tonight = Vec::new();
        for (id, tier) in stadiums {
            if self.srng().random_bool(EVENT_CHANCE[tier.index()]) {
                tonight.push(id);
            }
        }
        let s = &mut self.services;
        s.events_today = tonight;
        let st = &mut s.stats;
        st.avg_response_yesterday = if st.responses_today > 0 { st.response_ticks_today as f32 / st.responses_today as f32 } else { st.avg_response_yesterday };
        st.incidents_today = [0; 3];
        st.resolved_today = [0; 3];
        st.unanswered_today = 0;
        st.arrests_today = 0;
        st.response_ticks_today = 0;
        st.responses_today = 0;
        st.tickets_today = 0.0;
        st.pupils_today = 0;
        st.out_of_town_calls_today = 0;
        st.response_min_yesterday = std::mem::take(&mut st.response_min_today);
        st.saturated_min_yesterday = st.saturated_min_today;
        st.saturated_min_today = [0; 3];
        st.queue_max_today = st.queued;
        st.unanswered_why_today = Default::default();
        st.mutual_aid_today = 0;
        st.diverted_today = 0;
        st.chases_today = 0;
        st.chases_tagged_today = 0;
        st.chases_lost_today = 0;
        st.unreachable_today = 0;
        st.out_of_town_fees_today = 0.0;
        st.bills_paid_today = 0.0;
        st.bills_covered_today = 0.0;
        st.stadium_crowd_today = 0;
        st.upkeep_today = upkeep;
        st.education_bonus_today = bonus;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::IdleMind;

    fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
    }

    /// A long avenue with civic land along its north side and housing to
    /// the south, everything finished.
    fn town() -> World {
        town_with(WorldConfig::default().seed)
    }

    fn town_with(seed: u64) -> World {
        let mut w = World::new(WorldConfig { seed, ..WorldConfig::test_town() });
        w.economy.treasury = 1_000_000.0;
        w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-700.0, 120.0), Vec2::new(700.0, 120.0)], 1, 1);
        for x in [-600.0, -300.0, 0.0, 300.0, 600.0] {
            w.add_road(&[Vec2::new(x, 0.0), Vec2::new(x, 120.0)], 1, 1);
        }
        w.add_zone(square(-690.0, 8.0, 1380.0, 50.0), LandUse::Civic);
        w.add_zone(square(-690.0, -60.0, 1380.0, 52.0), LandUse::Residential);
        w
    }

    /// The vacant civic lot nearest `x` that fronts the avenue (y = 0).
    fn civic_lot(w: &World, x: f32) -> LotId {
        w.lots
            .iter()
            .filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.building.is_none() && l.car_access() && l.area() >= 420.0 && l.car_point.y.abs() < 12.0)
            .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
            .map(|(id, _)| id)
            .expect("a civic lot")
    }

    /// An ordinary finished building on the civic lot nearest `x`.
    fn building_at(w: &mut World, x: f32) -> BuildingId {
        let lot = civic_lot(w, x);
        w.build_on(lot);
        let id = w.lots[lot].building.unwrap();
        w.finish_construction(id);
        id
    }

    /// Homes and people: housing on the south side, filled by immigration.
    fn people(w: &mut World, n: usize) -> Vec<CitizenId> {
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable()).map(|(id, _)| id).take(8).collect();
        for l in lots {
            w.build_on(l);
            if let Some(b) = w.lots[l].building {
                w.finish_construction(b);
            }
        }
        for _ in 0..40 {
            if w.citizens.len() >= n {
                break;
            }
            w.immigrate();
        }
        assert!(w.citizens.len() >= n, "only {} citizens", w.citizens.len());
        w.citizens.keys().take(n).collect()
    }

    /// Put people on a building's payroll, at work if `inside`.
    fn staff(w: &mut World, b: BuildingId, who: &[CitizenId], inside: bool) {
        for &c in who {
            if let Some(old) = w.citizens[c].work {
                w.buildings[old].workers.retain(|x| *x != c);
            }
            w.citizens[c].work = Some(b);
            w.buildings[b].workers.push(c);
            if inside {
                let e = w.building_entrance(b).unwrap();
                let cz = &mut w.citizens[c];
                cz.activity = Activity::Working;
                cz.inside = Some(b);
                cz.pos = e;
                cz.next_decision_tick = u64::MAX / 2;
            }
        }
    }

    fn run_until(w: &mut World, max: usize, mut done: impl FnMut(&World) -> bool) -> usize {
        let mut minds = IdleMind;
        for t in 0..max {
            if done(w) {
                return t;
            }
            w.tick(&mut minds);
        }
        max
    }

    #[test]
    fn a_fire_is_called_in_dispatched_fought_and_put_out() {
        let mut w = town();
        let crew = people(&mut w, 8);
        let station = w.build_service(civic_lot(&w, -350.0), ServiceKind::Fire, Tier::Mid).unwrap();
        let desk = w.build_service(civic_lot(&w, -150.0), ServiceKind::Dispatch, Tier::Mid).unwrap();
        staff(&mut w, station, &crew[..6], false);
        staff(&mut w, desk, &crew[6..7], true);
        assert_eq!(w.crewed_units(station), 2);
        let target = building_at(&mut w, 100.0);
        assert!(w.services.coverage_of(w.buildings[target].lot, ServiceKind::Fire) > 0.5, "the target sits in the station's district");
        let id = w.start_fire(target).unwrap();
        w.drain_events();
        let mut seen = Vec::new();
        let mut minds = IdleMind;
        for _ in 0..600 {
            w.tick(&mut minds);
            for e in w.drain_events() {
                match e {
                    SimEvent::EmergencyCall(i) if i == id => seen.push("call"),
                    SimEvent::UnitDispatched(i) if i == id => seen.push("dispatch"),
                    SimEvent::UnitOnScene(i) if i == id => seen.push("on scene"),
                    SimEvent::IncidentResolved(i) if i == id => seen.push("resolved"),
                    _ => {}
                }
            }
            if !w.services.incident(id).unwrap().active() {
                break;
            }
            // While it drives, the engine is a car on the road.
            if w.services.incident(id).unwrap().stage == Stage::Dispatched {
                let uid = w.services.incident(id).unwrap().unit.unwrap();
                let car = w.services.units[uid].car.expect("a responding unit drives");
                assert!(w.traffic.cars.contains_key(car));
                assert_eq!(w.services.unit_of_car(car), Some(uid));
            }
        }
        let inc = w.services.incident(id).unwrap();
        assert_eq!(seen, vec!["call", "dispatch", "on scene", "resolved"]);
        assert_eq!(inc.outcome, Some(Outcome::Extinguished));
        assert_eq!(inc.dispatch_at.unwrap() - inc.reported.unwrap(), DISPATCH_ON_SHIFT, "a dispatcher at the desk is quick");
        assert_eq!(inc.dispatcher, Some(crew[6]));
        assert!(w.buildings.contains_key(target), "put out in time");
        assert!(w.buildings[target].condition > BURNED_DOWN);
        assert!(w.citizens[crew[6]].memories.iter().any(|m| m.text.contains("Took a 911 call")), "the dispatcher remembers the job");
        // The engine drives home and parks.
        let uid = inc.unit.unwrap();
        run_until(&mut w, 600, |w| w.services.units[uid].state == UnitState::Idle);
        assert_eq!(w.services.units[uid].state, UnitState::Idle);
        assert!(w.services.units[uid].car.is_none());
        assert!(w.services.stats.responses_today >= 1 && w.services.stats.avg_response() > 0.0);
    }

    #[test]
    fn without_a_dispatcher_the_job_goes_out_slower() {
        let mut w = town();
        let crew = people(&mut w, 8);
        let station = w.build_service(civic_lot(&w, -450.0), ServiceKind::Police, Tier::Mid).unwrap();
        staff(&mut w, station, &crew[..4], false);
        let target = building_at(&mut w, 150.0);
        // No dispatch centre: the regional line.
        assert_eq!(w.dispatcher(), (None, DISPATCH_REGIONAL));
        let slow = w.start_crime(target).unwrap();
        run_until(&mut w, 200, |w| w.services.incident(slow).unwrap().reported.is_some());
        let i = w.services.incident(slow).unwrap();
        let slow_delay = i.dispatch_at.unwrap() - i.reported.unwrap();
        // A centre with staff but nobody at the desk is on call; at the desk is quickest.
        let desk = w.build_service(civic_lot(&w, -150.0), ServiceKind::Dispatch, Tier::Low).unwrap();
        staff(&mut w, desk, &crew[5..6], false);
        assert_eq!(w.dispatcher().1, DISPATCH_ON_CALL);
        staff(&mut w, desk, &crew[6..7], true);
        assert_eq!(w.dispatcher(), (Some(crew[6]), DISPATCH_ON_SHIFT));
        let target2 = building_at(&mut w, 450.0);
        let fast = w.start_crime(target2).unwrap();
        run_until(&mut w, 200, |w| w.services.incident(fast).unwrap().reported.is_some());
        let i = w.services.incident(fast).unwrap();
        let fast_delay = i.dispatch_at.unwrap() - i.reported.unwrap();
        assert!(slow_delay > fast_delay * 5, "regional {slow_delay} vs desk {fast_delay}");
        assert_eq!(slow_delay, DISPATCH_REGIONAL);
        // Both get a car in the end, and each crime is closed one way or another.
        run_until(&mut w, 900, |w| !w.services.incident(slow).unwrap().active() && !w.services.incident(fast).unwrap().active());
        for id in [slow, fast] {
            let o = w.services.incident(id).unwrap().outcome;
            assert!(matches!(o, Some(Outcome::Arrest | Outcome::Escaped)), "{o:?}");
        }
    }

    #[test]
    fn a_service_on_a_built_lot_buys_it_out_but_not_another_service() {
        let mut w = town();
        let folk = people(&mut w, 3);
        let shop = building_at(&mut w, -250.0);
        let lot = w.buildings[shop].lot;
        staff(&mut w, shop, &folk[..1], false);
        let school = w.build_service(lot, ServiceKind::School, Tier::Mid).expect("the city takes the lot");
        assert!(!w.buildings.contains_key(shop), "the shop was pulled down");
        assert_eq!(w.lots[lot].building, Some(school));
        assert_ne!(w.citizens[folk[0]].work, Some(shop), "its staff were laid off");
        assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::Expropriated(b, l) if *b == shop && *l == lot)));
        let err = w.build_service(lot, ServiceKind::Police, Tier::Low).unwrap_err();
        assert!(err.contains("bulldoze it first"), "{err}");
        assert!(w.buildings.contains_key(school));
    }

    #[test]
    fn coverage_follows_road_distance_and_tier() {
        let mut w = town();
        let lot = civic_lot(&w, -560.0);
        let station = w.build_service(lot, ServiceKind::Fire, Tier::Low).unwrap();
        let r = ServiceKind::Fire.radius(Tier::Low);
        let at = w.lots[lot].car_point;
        // Lots on the avenue: full coverage close by, fading over the last
        // COVERAGE_FADE metres of road, none beyond the radius.
        let mut checked = 0;
        for (id, l) in w.lots.iter() {
            if l.car_segment.is_none() || l.car_point.y.abs() > 12.0 {
                continue;
            }
            let d = l.car_point.distance(at);
            let cov = w.services.coverage_of(id, ServiceKind::Fire);
            if d < r - COVERAGE_FADE - 5.0 {
                assert!(cov > 0.99, "lot {d:.0} m away should be covered: {cov}");
            } else if d > r + 5.0 {
                assert_eq!(cov, 0.0, "lot {d:.0} m away is out of reach");
            } else if d > r - COVERAGE_FADE + 5.0 && d < r - 5.0 {
                let want = (r - d) / COVERAGE_FADE;
                assert!((cov - want).abs() < 0.05, "lot {d:.0} m: {cov} vs {want}");
            }
            checked += 1;
        }
        assert!(checked > 10);
        // Road distance, not straight line: a lot just across the avenue's
        // end block is close as the crow flies but far by road.
        assert_eq!(w.services.coverage_of(lot, ServiceKind::Police), 0.0, "only the kind placed covers");
        // A higher tier reaches further.
        let far = w.lots.iter().filter(|(_, l)| l.car_segment.is_some() && l.car_point.y.abs() < 12.0).map(|(id, l)| (id, l.car_point.distance(at))).filter(|(_, d)| *d > r + 50.0 && *d < ServiceKind::Fire.radius(Tier::High) - COVERAGE_FADE - 50.0).map(|(id, _)| id).next().expect("a lot between the two radii");
        assert_eq!(w.services.coverage_of(far, ServiceKind::Fire), 0.0);
        w.demolish(station);
        w.lots[lot].building = None;
        w.build_service(lot, ServiceKind::Fire, Tier::High).unwrap();
        assert!(w.services.coverage_of(far, ServiceKind::Fire) > 0.99);
        // Coverage cuts the fire risk the hourly roll uses.
        let risk = |cov: f32| FIRE_RISK[1] * (1.0 - FIRE_COVER_CUT * cov);
        assert!(risk(1.0) < risk(0.0) * 0.35);
    }

    #[test]
    fn a_sick_citizen_is_taken_to_hospital() {
        let mut w = town();
        let folk = people(&mut w, 6);
        let hospital = w.build_service(civic_lot(&w, -450.0), ServiceKind::Health, Tier::Mid).unwrap();
        staff(&mut w, hospital, &folk[..4], false);
        assert_eq!(w.services.units.values().filter(|u| u.station == hospital).count(), 2);
        // Somebody at home falls ill.
        let patient = folk[5];
        let home = w.citizens[patient].home.unwrap();
        {
            let e = w.building_entrance(home).unwrap();
            let c = &mut w.citizens[patient];
            c.activity = Activity::AtHome;
            c.inside = Some(home);
            c.pos = e;
        }
        let id = w.fall_ill(patient).unwrap();
        assert!(w.is_patient(patient));
        let mut stages = Vec::new();
        let mut minds = IdleMind;
        for _ in 0..900 {
            w.tick(&mut minds);
            let s = w.services.incident(id).unwrap().stage;
            if s != Stage::Unreported && stages.last() != Some(&s) {
                stages.push(s);
            }
            if s == Stage::Resolved {
                break;
            }
        }
        assert_eq!(stages, vec![Stage::Reported, Stage::Dispatched, Stage::OnScene, Stage::Transporting, Stage::Resolved], "{stages:?}");
        assert_eq!(w.services.incident(id).unwrap().outcome, Some(Outcome::Admitted));
        let c = &w.citizens[patient];
        assert_eq!(c.inside, Some(hospital), "in a bed");
        assert!(matches!(c.activity, Activity::Visiting(h) if h == hospital));
        assert!(c.memories.iter().any(|m| m.text.contains("ambulance took me")));
        assert_eq!(w.services.admissions.len(), 1);
        assert!(w.is_patient(patient), "still a patient while admitted");
        assert!(c.next_decision_tick >= w.clock.tick + STAY_HOURS.0 * 60 - 5);
    }

    #[test]
    fn an_unanswered_fire_spreads_and_burns_down() {
        let mut w = town();
        // A terrace of buildings shoulder to shoulder, no fire station.
        let lots: Vec<LotId> = {
            let mut v: Vec<(f32, LotId)> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.car_access() && l.center.x > 0.0 && l.center.x < 290.0).map(|(id, l)| (l.center.x, id)).collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            v.into_iter().map(|(_, id)| id).collect()
        };
        assert!(lots.len() >= 4, "{} lots", lots.len());
        let mut row = Vec::new();
        for &l in &lots {
            w.build_on(l);
            let b = w.lots[l].building.unwrap();
            w.finish_construction(b);
            row.push(b);
        }
        let first = row[lots.len() / 2];
        let id = w.start_fire(first).unwrap();
        let mut spread = 0;
        let mut burned = Vec::new();
        let mut minds = IdleMind;
        for _ in 0..(2 * 1440) {
            w.tick(&mut minds);
            for e in w.drain_events() {
                match e {
                    SimEvent::FireSpread(_) => spread += 1,
                    SimEvent::BurnedDown(b, _) => burned.push(b),
                    _ => {}
                }
            }
        }
        let inc = w.services.incident(id).unwrap();
        assert!(inc.dispatched.is_none(), "there is nobody to send");
        assert_eq!(inc.outcome, Some(Outcome::BurnedDown));
        assert!(burned.contains(&first));
        assert!(!w.buildings.contains_key(first));
        let lot = lots[lots.len() / 2];
        assert_eq!(w.lots[lot].building, None, "the lot is free again");
        assert!(spread > 0, "a fire nobody fights jumps to the neighbours");
        assert!(w.services.stats.burned_down_total >= 1);
        // Rubble keeps development off the lot for a while.
        assert!(w.services.is_rubble(lot));
    }

    #[test]
    fn services_cost_upkeep_and_stadiums_draw_a_paying_crowd() {
        let mut w = town();
        let folk = people(&mut w, 30);
        let big = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.car_access()).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, l)| (id, l.area())).unwrap();
        // A stadium needs room: a small lot is refused with the reason.
        let small = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.car_access() && l.area() < ServiceKind::Stadium.min_area(Tier::High)).map(|(id, _)| id).next().unwrap();
        assert!(w.build_service(small, ServiceKind::Stadium, Tier::High).unwrap_err().contains("too small"));
        assert!(big.1 >= ServiceKind::Stadium.min_area(Tier::Low), "largest civic lot {:.0}", big.1);
        let stadium = w.build_service(big.0, ServiceKind::Stadium, Tier::Low).unwrap();
        let school = w.build_service(civic_lot(&w, 0.0), ServiceKind::School, Tier::Mid).unwrap();
        let _ = school;
        for &c in &folk {
            let h = w.citizens[c].home.unwrap();
            let e = w.building_entrance(h).unwrap();
            let cz = &mut w.citizens[c];
            cz.activity = Activity::AtHome;
            cz.inside = Some(h);
            cz.pos = e;
            cz.savings = 200.0;
        }
        w.services.events_today = vec![stadium];
        let before = w.economy.treasury;
        while w.clock.minute_of_day() != EVENT_MINUTE - 1 {
            w.clock.advance();
        }
        w.tick(&mut IdleMind);
        assert!(w.services.stats.stadium_crowd_today > 0, "fans set off");
        assert!(w.economy.ledger.today_of(Flow::Tickets) > 0.0);
        assert!(w.citizens.values().any(|c| matches!(&c.activity, Activity::Traveling { to: Some(b), .. } | Activity::Driving { to: Some(b), .. } if *b == stadium)));
        let _ = before;
        w.daily_services();
        assert!(w.economy.ledger.today_of(Flow::ServiceUpkeep) < 0.0);
    }

    #[test]
    fn children_go_to_a_school_that_covers_their_home_and_staff_work_round_the_clock() {
        let mut w = town();
        people(&mut w, 40);
        let kids: Vec<CitizenId> = w.citizens.iter().filter(|(_, c)| c.goes_to_school()).map(|(id, _)| id).collect();
        assert!(!kids.is_empty(), "families with school-age children arrived");
        let school = w.build_service(civic_lot(&w, -150.0), ServiceKind::School, Tier::Mid).unwrap();
        for &k in &kids {
            let h = w.citizens[k].home.unwrap();
            let e = w.building_entrance(h).unwrap();
            let c = &mut w.citizens[k];
            c.activity = Activity::AtHome;
            c.inside = Some(h);
            c.pos = e;
        }
        while w.clock.minute_of_day() != SCHOOL_MINUTE - 1 {
            w.clock.advance();
        }
        w.tick(&mut IdleMind);
        assert!(w.services.stats.pupils_today > 0);
        let going = kids.iter().filter(|k| matches!(&w.citizens[**k].activity, Activity::Traveling { to: Some(b), .. } | Activity::Driving { to: Some(b), .. } if *b == school)).count();
        assert!(going > 0, "children set off for school");
        // Emergency services staff in shifts round the clock; schools by day.
        let fire = w.build_service(civic_lot(&w, 350.0), ServiceKind::Fire, Tier::Low).unwrap();
        let shifts: std::collections::HashSet<u8> = (0..30u64).map(|seed| w.buildings[fire].shift_for(seed).open).collect();
        assert!(shifts.len() >= 3, "three shifts: {shifts:?}");
        assert!(!w.buildings[school].opening_hours().contains(22));
    }

    /// A robbery with a suspect, answered and arrested: with a jail the
    /// suspect serves days there; without one they are held a few hours at
    /// the station and let go early.
    fn arrest_case(with_jail: bool) -> (World, CitizenId, u32) {
        // The arrest is a dice roll: take the first seed where it happens.
        for seed in 1..40 {
            let case = arrest_case_seeded(with_jail, seed);
            if case.0.services.incident(case.2).unwrap().outcome == Some(Outcome::Arrest) {
                return case;
            }
        }
        panic!("no arrest in 40 seeds");
    }

    fn arrest_case_seeded(with_jail: bool, seed: u64) -> (World, CitizenId, u32) {
        let mut w = town_with(seed);
        let folk = people(&mut w, 8);
        let station = w.build_service(civic_lot(&w, -350.0), ServiceKind::Police, Tier::Mid).unwrap();
        staff(&mut w, station, &folk[..4], false);
        if with_jail {
            w.build_service(civic_lot(&w, -150.0), ServiceKind::Jail, Tier::Low).unwrap();
        }
        let shop = building_at(&mut w, 100.0);
        let suspect = folk[6];
        let id = w.report_crime(CrimeKind::Theft, shop, Some(suspect), Some(folk[7])).unwrap();
        assert_eq!(w.services.incident(id).unwrap().title(), "theft");
        // Make the arrest certain for the test.
        let mut minds = IdleMind;
        for _ in 0..400 {
            w.tick(&mut minds);
            if w.services.incident(id).unwrap().stage == Stage::OnScene {
                w.services.incident_mut(id).unwrap().arrived = Some(w.services.incident(id).unwrap().started);
            }
            if !w.services.incident(id).unwrap().active() {
                break;
            }
        }
        (w, suspect, id)
    }

    #[test]
    fn arrests_go_to_jail_or_are_let_go_early_without_one() {
        let (w, suspect, id) = arrest_case(true);
        let inc = w.services.incident(id).unwrap();
        {
            let c = w.services.custody.iter().find(|c| c.citizen == suspect).expect("in custody");
            assert!(c.jailed && c.until >= w.clock.tick + SENTENCE_DAYS.0 * 1440 - 2);
            assert_eq!(w.services.jail_occupancy().iter().map(|o| o.1).sum::<u32>(), 1);
            assert_eq!(w.services.arrests_of(suspect), 1);
            assert!(inc.people.iter().any(|p| p.role == "suspect" && p.taken_to.as_deref().is_some_and(|t| t.contains("jail"))));
        }
        let (mut w, suspect, id) = arrest_case(false);
        {
            assert_eq!(w.services.incident(id).unwrap().outcome, Some(Outcome::Arrest));
            let c = *w.services.custody.iter().find(|c| c.citizen == suspect).expect("held");
            assert!(!c.jailed && c.until <= w.clock.tick + HELD_HOURS * 60);
            run_until(&mut w, (HELD_HOURS * 60 + 30) as usize, |w| !w.services.in_custody(suspect));
            assert!(!w.services.in_custody(suspect));
            assert_eq!(w.services.stats.released_early_total, 1);
        }
    }

    #[test]
    fn with_no_station_but_a_dispatcher_help_comes_from_out_of_town_for_a_fee() {
        let mut w = town();
        let folk = people(&mut w, 3);
        let desk = w.build_service(civic_lot(&w, -150.0), ServiceKind::Dispatch, Tier::Low).unwrap();
        staff(&mut w, desk, &folk[..1], true);
        let target = building_at(&mut w, 100.0);
        let before = w.economy.treasury;
        let id = w.start_fire(target).unwrap();
        run_until(&mut w, 600, |w| !w.services.incident(id).unwrap().active());
        let inc = w.services.incident(id).unwrap();
        assert!(inc.outside, "an out-of-town engine came");
        assert_eq!(inc.travel_ticks(), Some(OUT_OF_TOWN_TRAVEL));
        assert_eq!(inc.outcome, Some(Outcome::Extinguished));
        assert!(inc.fee > 0.0 && w.economy.treasury <= before - inc.fee);
        assert!(!inc.damage.is_empty() && inc.damage[0].lost > 0.0, "the fire did some damage first");
        assert!(inc.people.iter().any(|p| p.role == "dispatcher"));
    }

    /// A police post (one car, two officers aboard), a 911 desk with a
    /// dispatcher at it, shops along the avenue, and no random incidents.
    fn one_car_town() -> (World, BuildingId, Vec<CitizenId>) {
        let mut w = town();
        w.config.emergencies = false;
        w.config.society = false;
        let folk = people(&mut w, 10);
        let station = w.build_service(civic_lot(&w, -600.0), ServiceKind::Police, Tier::Low).unwrap();
        let desk = w.build_service(civic_lot(&w, -450.0), ServiceKind::Dispatch, Tier::Mid).unwrap();
        staff(&mut w, station, &folk[..2], false);
        staff(&mut w, desk, &folk[2..3], true);
        assert_eq!(w.crewed_units(station), 1, "two officers crew one car");
        (w, station, folk)
    }

    /// A crime called in at once.
    fn crime_now(w: &mut World, kind: CrimeKind, at: BuildingId, suspect: Option<CitizenId>) -> u32 {
        let id = w.report_crime(kind, at, suspect, None).unwrap();
        let t = w.clock.tick;
        w.services.incident_mut(id).unwrap().report_at = t;
        id
    }

    fn stage(w: &World, id: u32) -> Stage {
        w.services.incident(id).unwrap().stage
    }

    #[test]
    fn free_police_patrol_and_answer_from_the_beat() {
        let (mut w, station, _) = one_car_town();
        let shop = building_at(&mut w, 450.0);
        run_until(&mut w, 60, |w| w.services.units.values().any(|u| u.station == station && u.state == UnitState::Patrol));
        let uid = w.services.units.iter().find(|(_, u)| u.station == station && u.kind == UnitKind::PoliceCar).map(|(id, _)| id).unwrap();
        assert_eq!(w.services.units[uid].state, UnitState::Patrol, "a free crewed car goes on patrol");
        assert!(!w.services.units[uid].busy(), "on patrol is free for calls");
        assert_eq!(w.services.stats.on_patrol, 1);
        // Out on the beat (parked or driving), not in the station.
        run_until(&mut w, 200, |w| w.services.units[uid].car.is_none() && w.services.units[uid].state == UnitState::Patrol);
        let beat = w.services.units[uid].pos;
        let home = w.building_lot(station).unwrap().car_point;
        assert!(beat.distance(home) > 60.0, "parked on a beat {beat} away from the station {home}");
        let id = crime_now(&mut w, CrimeKind::Robbery, shop, None);
        run_until(&mut w, 30, |w| stage(w, id) == Stage::Dispatched);
        assert_eq!(w.services.incident(id).unwrap().unit, Some(uid));
        let car = w.services.units[uid].car.expect("driving with lights and sirens");
        assert!(w.traffic.cars[car].pos.distance(beat) < 40.0, "it set off from the beat, not the station");
        run_until(&mut w, 120, |w| stage(w, id) != Stage::Dispatched);
        assert_eq!(stage(&w, id), Stage::OnScene);
        assert_eq!(w.services.stats.response_min_today[IncidentKind::Crime.index()].len(), 1);
    }

    #[test]
    fn queued_calls_go_out_most_urgent_first_the_moment_a_car_clears() {
        let (mut w, _, folk) = one_car_town();
        let shops = [building_at(&mut w, 150.0), building_at(&mut w, 300.0), building_at(&mut w, 450.0)];
        let first = crime_now(&mut w, CrimeKind::Theft, shops[0], None);
        run_until(&mut w, 200, |w| stage(w, first) == Stage::OnScene);
        assert_eq!(stage(&w, first), Stage::OnScene);
        // While the only car is busy on scene: vandalism, then a robbery.
        let minor = crime_now(&mut w, CrimeKind::Vandalism, shops[1], None);
        run_until(&mut w, 2, |_| false);
        let violent = crime_now(&mut w, CrimeKind::Robbery, shops[2], Some(folk[9]));
        run_until(&mut w, 10, |_| false);
        assert_eq!((stage(&w, minor), stage(&w, violent)), (Stage::Reported, Stage::Reported), "both wait in the queue");
        assert_eq!(w.services.stats.queued[IncidentKind::Crime.index()], 2);
        assert!(w.services.stats.saturated_since[IncidentKind::Crime.index()].is_some(), "every police car is busy");
        run_until(&mut w, 200, |w| !w.services.incident(first).unwrap().active());
        let cleared = w.services.incident(first).unwrap().resolved.unwrap();
        run_until(&mut w, 2, |_| false);
        let v = w.services.incident(violent).unwrap();
        assert_eq!(v.stage, Stage::Dispatched, "the robbery goes out first, though called in later");
        assert!(v.dispatched.unwrap() <= cleared + 1, "re-dispatched the moment the car cleared ({} vs {cleared})", v.dispatched.unwrap());
        assert_eq!(stage(&w, minor), Stage::Reported, "the vandalism waits its turn");
        assert!(w.services.stats.queue_max_today[IncidentKind::Crime.index()] >= 2 && w.services.stats.saturated_min_today[IncidentKind::Crime.index()] > 0);
        run_until(&mut w, 400, |w| stage(w, minor) != Stage::Reported);
        assert_eq!(stage(&w, minor), Stage::Dispatched, "and then goes out too");
    }

    #[test]
    fn a_violent_crime_pulls_a_car_off_a_theft() {
        let (mut w, _, _) = one_car_town();
        let near = building_at(&mut w, -300.0);
        let far = building_at(&mut w, 600.0);
        let theft = crime_now(&mut w, CrimeKind::Theft, far, None);
        run_until(&mut w, 20, |w| stage(w, theft) == Stage::Dispatched);
        let uid = w.services.incident(theft).unwrap().unit.unwrap();
        let assault = crime_now(&mut w, CrimeKind::Assault, near, None);
        run_until(&mut w, 10, |w| stage(w, assault) == Stage::Dispatched);
        assert_eq!(w.services.incident(assault).unwrap().unit, Some(uid), "the car on its way to the theft is sent to the assault");
        assert_eq!(stage(&w, theft), Stage::Reported, "the theft goes back in the queue");
        assert_eq!(w.services.stats.diverted_today, 1);
    }

    #[test]
    fn with_every_car_busy_a_long_wait_brings_mutual_aid() {
        let (mut w, _, _) = one_car_town();
        let shops = [building_at(&mut w, 150.0), building_at(&mut w, 450.0)];
        let long = crime_now(&mut w, CrimeKind::Theft, shops[0], None);
        run_until(&mut w, 200, |w| stage(w, long) == Stage::OnScene);
        w.services.incident_mut(long).unwrap().work_left = 1_000.0;
        let before = w.economy.treasury;
        let waiting = crime_now(&mut w, CrimeKind::Robbery, shops[1], None);
        run_until(&mut w, MUTUAL_AID_WAIT as usize - 5, |_| false);
        assert_eq!(stage(&w, waiting), Stage::Reported, "queued at first");
        run_until(&mut w, 20, |w| stage(w, waiting) != Stage::Reported);
        let i = w.services.incident(waiting).unwrap();
        assert!(i.outside && i.unit_name.as_deref().is_some_and(|n| n.contains("mutual aid")), "{:?}", i.unit_name);
        assert_eq!(i.arrive_at, Some(i.dispatched.unwrap() + MUTUAL_AID_TRAVEL));
        assert!(w.economy.treasury < before, "billed");
        assert_eq!(w.services.stats.mutual_aid_today, 1);
    }

    #[test]
    fn crashes_past_the_map_edge_are_the_next_towns_and_a_pile_up_is_one_scene() {
        let (mut w, _, folk) = one_car_town();
        let near = building_at(&mut w, 150.0);
        let far = Vec2::new(crate::landscape::MAP_EDGE + 500.0, 0.0);
        assert!(w.report_crash(far, near, &folk[5..6], &folk[6..7], None, None).is_none(), "out past the edge: not a call of ours");
        assert!(w.services.incidents.is_empty());
        let here = Vec2::new(150.0, 0.0);
        let (a, _) = w.report_crash(here, near, &folk[5..6], &[], None, None).unwrap();
        let (b, _) = w.report_crash(here + Vec2::new(20.0, 0.0), near, &folk[7..8], &[], None, None).unwrap();
        assert_eq!(a, b, "a second crash at the same spot joins the open scene");
    }

    #[test]
    fn crash_scenes_take_as_long_as_the_case_and_a_crash_wave_saturates_the_police() {
        let (mut w, _, folk) = one_car_town();
        let spots: Vec<BuildingId> = [-300.0, -150.0, 0.0, 150.0, 300.0, 450.0].iter().map(|&x| building_at(&mut w, x)).collect();
        // A written-off car alone: a short clear-up; someone hurt: longer; a drunk to arrest: longest.
        let (clear, _) = w.report_crash(Vec2::new(-300.0, 0.0), spots[0], &folk[4..5], &[], None, None).unwrap();
        let (hurt, _) = w.report_crash(Vec2::new(-150.0, 0.0), spots[1], &folk[5..6], &folk[5..6], None, None).unwrap();
        let (drunk, _) = w.report_crash(Vec2::new(0.0, 0.0), spots[2], &folk[6..7], &[], Some(folk[6]), None).unwrap();
        let work = |w: &World, id: u32| w.services.incident(id).unwrap().work_left;
        assert_eq!((work(&w, clear), work(&w, hurt), work(&w, drunk)), (CRASH_WORK_CLEAR, CRASH_WORK, CRASH_WORK_ARREST));
        // A wave of crashes for the one car: every unit busy past the alert threshold.
        let mut k = 0;
        for t in 0..(SATURATED_ALERT_MIN as usize + 60) {
            if t % 15 == 0 {
                let x = [150.0, 300.0, 450.0][k % 3];
                w.report_crash(Vec2::new(x, 0.0), spots[3 + k % 3], &folk[7..8], &folk[7..8], None, None);
                k += 1;
            }
            run_until(&mut w, 1, |_| false);
        }
        let k = IncidentKind::Crash.slot();
        assert!(w.services.stats.saturated_min_today[k] >= SATURATED_ALERT_MIN, "saturated {} min", w.services.stats.saturated_min_today[k]);
        assert!(w.services.stats.queue_max_today[k] >= 2);
    }
    // ── [services] stations that go, civic fires, through traffic ─────────

    /// Is `p` at one of the town's road ends (where a vehicle may leave)?
    fn at_road_end(w: &World, p: Vec2) -> bool {
        w.roads.leaf_nodes().into_iter().any(|n| w.roads.node(n).pos.distance(p) < 40.0)
    }

    #[test]
    fn a_station_bulldozed_while_its_car_is_on_a_call_hands_the_call_back_and_drives_off_the_map() {
        let (mut w, station, _) = one_car_town();
        let shop = building_at(&mut w, 450.0);
        let id = crime_now(&mut w, CrimeKind::Robbery, shop, None);
        run_until(&mut w, 60, |w| stage(w, id) == Stage::Dispatched);
        assert_eq!(stage(&w, id), Stage::Dispatched);
        let uid = w.services.incident(id).unwrap().unit.unwrap();
        let car = w.services.units[uid].car.unwrap();
        assert!(!World::is_through_car(&w.traffic.cars[car]), "a police car is not through traffic");
        w.demolish_with(station, None, None);
        assert!(!w.buildings.contains_key(station), "a civic structure comes down at once");
        run_until(&mut w, 12, |_| false);
        let i = w.services.incident(id).unwrap();
        assert!(i.unit.is_none() && matches!(i.stage, Stage::Reported | Stage::Dispatched | Stage::OnScene | Stage::Resolved), "the call went back to the queue: {:?} unit {:?}", i.stage, i.unit);
        assert!(i.unit.is_none_or(|u| w.services.units.get(u).is_some_and(|x| !x.leaving)), "no call names a unit leaving town");
        let u = &w.services.units[uid];
        assert!(u.leaving && !u.lights(), "the car leaves town without lights");
        let mut last = w.traffic.cars[car].pos;
        assert!(!at_road_end(&w, last), "it was still in town when its station went");
        let mut gone_at = None;
        for _ in 0..600 {
            w.tick(&mut IdleMind);
            match w.services.units.get(uid).and_then(|u| u.car).and_then(|c| w.traffic.cars.get(c)) {
                Some(c) => last = c.pos,
                None if !w.services.units.contains_key(uid) => {
                    gone_at = Some(last);
                    break;
                }
                None => {}
            }
        }
        let at = gone_at.expect("the unit left the world");
        assert!(at_road_end(&w, at), "it left at a road end, not mid-street: {at}");
        // The call was not orphaned: it ends one way or another.
        run_until(&mut w, 3000, |w| !w.services.incident(id).unwrap().active());
        assert!(!w.services.incident(id).unwrap().active(), "the call ends");
    }

    #[test]
    fn a_call_whose_unit_vanished_goes_back_in_the_queue() {
        let (mut w, _, _) = one_car_town();
        let shop = building_at(&mut w, 450.0);
        let id = crime_now(&mut w, CrimeKind::Theft, shop, None);
        run_until(&mut w, 60, |w| stage(w, id) == Stage::Dispatched);
        let uid = w.services.incident(id).unwrap().unit.unwrap();
        // However it happened (a stale save, a bug elsewhere): the unit is gone.
        if let Some(car) = w.services.units.remove(uid).and_then(|u| u.car) {
            w.traffic.remove_car(car);
        }
        run_until(&mut w, 2, |_| false);
        let i = w.services.incident(id).unwrap();
        assert!(i.unit != Some(uid), "the incident no longer names the vanished unit");
        assert!(!i.active() || i.unit.is_none_or(|u| w.services.units.contains_key(u)), "{:?}", i.stage);
    }

    #[test]
    fn a_civic_fire_guts_the_station_and_the_city_repairs_it() {
        let (mut w, station, _) = one_car_town();
        w.config.emergencies = true;
        w.economy.treasury = 50_000.0;
        w.buildings[station].condition = BURNED_DOWN + 0.01;
        let id = w.start_fire(station).unwrap();
        run_until(&mut w, 400, |w| !w.services.incident(id).unwrap().active());
        let i = w.services.incident(id).unwrap();
        assert!(!i.active(), "the fire ends");
        assert!(w.buildings.contains_key(station), "the station still stands");
        assert_eq!(i.outcome, Some(Outcome::Gutted));
        assert!(w.buildings[station].is_derelict() && w.crewed_units(station) == 0, "gutted: out of service");
        assert!(!w.services.rubble.iter().any(|r| Some(r.0) == w.buildings.get(station).map(|b| b.lot)), "no rubble on a standing station");
        let money = w.economy.treasury;
        // Two midnights of repairs bring it back past the derelict line.
        for _ in 0..2 {
            run_until(&mut w, 1500, |w| w.clock.minute_of_day() == 0);
            run_until(&mut w, 1, |_| false);
        }
        assert!(!w.buildings[station].is_derelict(), "repaired: condition {}", w.buildings[station].condition);
        assert!(w.economy.treasury < money, "repairs cost money");
    }

    #[test]
    fn civic_structures_do_not_decay() {
        let (mut w, station, _) = one_car_town();
        let before = w.buildings[station].condition;
        for _ in 0..3 {
            run_until(&mut w, 1500, |w| w.clock.minute_of_day() == 0);
            run_until(&mut w, 1, |_| false);
        }
        assert!(w.buildings[station].condition >= before, "{} -> {}", before, w.buildings[station].condition);
    }
}
