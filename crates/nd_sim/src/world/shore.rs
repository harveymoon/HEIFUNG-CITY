//! [waterfront] The world side of the shore (`crate::shore`): marking the
//! lots that meet the water, the public beaches (access, weather, crowds,
//! contamination, what they do for the land and the people round them),
//! the private jetties of businesses on the water and the loads their
//! coasters bring in.
//!
//! Hooks elsewhere (one line each): `plan.rs` cuts parcels to the water
//! (`cut_parcels`), marks lots after a rebuild (`mark_shore_lots`) and lets
//! the Zone tool paint beaches (`paint_parcels_with`); `park.rs` lays a
//! beach out as sand (`plan_parks`); `mod.rs` runs [`World::hourly_shore`]
//! at :33 and [`World::daily_shore`] at midnight; `develop.rs` splats the
//! premiums into the land value; `market.rs` imports through a private
//! quay first; `places.rs` keeps people off shut beaches and draws them to
//! open ones in the heat; `tourism.rs` lists beaches as sights; `walk.rs`
//! lets a beach gate only into a park.
//!
//! Deterministic: no draw from the world's RNG; ties go to the lower id.

use super::*;
use crate::shore::{self, BeachState, Jetty, JettyKind};

/// How far a tourist's visit to a beach may land from its boardwalk.
const BEACH_SPOT_REACH: f32 = 400.0;

impl World {
    // ── lots on the water ──────────────────────────────────────────────

    /// Which parcels the plan paints as beach: the newest stroke over a
    /// parcel wins (a later zone or stroke of anything else clears it).
    pub(super) fn parcel_beaches(&self, plan: &Plan) -> Vec<bool> {
        let mut out = vec![false; self.parcels.len()];
        for g in plan.gestures.values() {
            match g {
                Gesture::Zone { polygon, .. } => {
                    if polygon.len() >= 3 && !crate::geom::self_intersects(polygon) {
                        for i in self.parcels.in_polygon(polygon) {
                            out[i] = false;
                        }
                    }
                }
                Gesture::ParcelZone { parcels, land_use, industry } => {
                    let beach = *land_use == Some(LandUse::Park) && industry.as_deref() == Some(shore::BEACH_SUBZONE);
                    for k in parcels {
                        if let Some(i) = self.parcels.find(k) {
                            out[i] = beach && self.parcels.parcels[i].water_frontage >= crate::zoning::LotShore::MIN_FRONTAGE;
                        }
                    }
                }
                Gesture::Road { .. } | Gesture::Roundabout { .. } => {}
            }
        }
        out
    }

    /// Parcels the effective plan (with the active project) paints as beach,
    /// for the zoning layer and the hover.
    pub fn planned_beaches(&self) -> Vec<bool> {
        self.parcel_beaches(self.effective_plan())
    }

    /// Why a parcel cannot be a public beach, or None.
    pub fn beach_refusal(&self, i: usize) -> Option<&'static str> {
        let p = self.parcels.parcels.get(i)?;
        if let Some(why) = self.parcel_refusal(i, Some(LandUse::Park)) {
            return Some(why);
        }
        if p.water_frontage < crate::zoning::LotShore::MIN_FRONTAGE {
            return Some("a beach must run down to the water");
        }
        None
    }

    /// Every lot's frontage on the water and which way it lies, and the
    /// beaches the master plan paints. After lots are rebuilt or loaded.
    pub(super) fn mark_shore_lots(&mut self) {
        let water = self.water_mask();
        let beaches = self.parcel_beaches(&self.master);
        let mut beach_lots: std::collections::HashSet<LotId> = Default::default();
        for (i, p) in self.parcels.parcels.iter().enumerate() {
            if beaches.get(i).copied().unwrap_or(false) {
                if let Some(l) = p.lot {
                    beach_lots.insert(l);
                }
            }
        }
        for (id, l) in self.lots.iter_mut() {
            let (frontage, seaward) = if water.is_empty() { (0.0, Vec2::ZERO) } else { water.shoreline(&l.polygon) };
            l.shore.frontage = frontage;
            l.shore.seaward = seaward;
            l.shore.beach = l.land_use == LandUse::Park && beach_lots.contains(&id) && l.shore.is_waterfront();
        }
    }

    /// [waterfront] Cut parcels down to the water along roads near it (on
    /// for cities started since; a save from before can switch it on). The
    /// parcels are re-cut now; lots follow at the next implement, and a
    /// building whose parcel changes shape then is pulled down like any
    /// other the works reshape.
    pub fn set_waterfront_parcels(&mut self, on: bool) {
        if self.shore.reach_water != on {
            self.shore.reach_water = on;
            self.recut_parcels();
        }
    }

    // ── beaches ────────────────────────────────────────────────────────

    /// The beach on a lot (its park building and state), if it is one.
    pub fn beach_on(&self, lot: LotId) -> Option<(BuildingId, &BeachState)> {
        let b = self.lots.get(lot).filter(|l| l.shore.beach)?.building?;
        self.shore.beaches.get(&b).map(|s| (b, s))
    }

    /// Can people go to the park on this lot now? Always for an ordinary
    /// park; a beach must be open, reachable and not deserted by the weather.
    pub fn park_open_for_visits(&self, lot: LotId) -> bool {
        match self.lots.get(lot) {
            Some(l) if l.shore.beach => self.beach_on(lot).is_some_and(|(_, s)| s.usable() && s.appeal > 0.08),
            _ => true,
        }
    }

    /// What draws people to the park on this lot beyond an ordinary park:
    /// a beach in good weather.
    pub fn beach_pull(&self, lot: LotId) -> f32 {
        self.beach_on(lot).map_or(0.0, |(_, s)| if s.usable() { s.appeal * 0.8 - if s.crowded() { 0.3 } else { 0.0 } } else { 0.0 })
    }

    /// How inviting a beach is now for the weather and the hour.
    pub fn beach_appeal_now(&self) -> f32 {
        let hour = self.clock.minute_of_day() as f32 / 60.0;
        shore::beach_weather(self.weather.temp_c, self.weather.cloud, self.weather.spell, hour)
    }

    /// How inviting a beach is on a typical afternoon this week (for the
    /// tourists deciding to come, and the pressures).
    pub fn beach_appeal_season(&self) -> f32 {
        shore::beach_weather(self.weather.temp_at(14.0), self.weather.cloud.min(0.5), crate::weather::Spell::Normal, 14.0)
    }

    /// Worst contamination over a beach: the sand, and the water just off it.
    fn beach_contamination(&self, lot: &Lot) -> f32 {
        let c = &self.contamination;
        let mut worst = c.over(lot.center, &lot.polygon);
        let sea = lot.shore.seaward;
        if sea != Vec2::ZERO {
            let edge = lot.polygon.iter().copied().max_by(|a, b| a.dot(sea).total_cmp(&b.dot(sea))).unwrap_or(lot.center);
            for k in [8.0f32, 25.0, 50.0] {
                worst = worst.max(c.at(edge + sea * k));
            }
        }
        worst
    }

    /// Re-read which beaches can be walked to, when the walk graph changed:
    /// from the beach's own nodes over the graph to a pavement or path
    /// node. A beach gates only into parks (`walk.rs`), so that is a
    /// street or path it fronts, or a park that is reached.
    fn refresh_beach_access(&mut self) {
        if self.shore.access_version == self.walk.version && self.shore.beaches.values().all(|b| b.capacity > 0) {
            return;
        }
        self.shore.access_version = self.walk.version;
        let mut by_lot: HashMap<LotId, Vec<u32>> = HashMap::new();
        let beach_lots: Vec<LotId> = self.shore.beaches.keys().filter_map(|b| self.buildings.get(*b).map(|x| x.lot)).collect();
        if beach_lots.is_empty() {
            return;
        }
        for (i, n) in self.walk.nodes.iter().enumerate() {
            if let Some(l) = n.lot {
                if beach_lots.contains(&l) {
                    by_lot.entry(l).or_default().push(i as u32);
                }
            }
        }
        let ids: Vec<BuildingId> = self.shore.beaches.keys().copied().collect();
        for b in ids {
            let Some(lot) = self.buildings.get(b).map(|x| x.lot) else { continue };
            let starts = by_lot.get(&lot).cloned().unwrap_or_default();
            let reached = self.walk_reaches_street(&starts, 4000);
            let cap = self.lots.get(lot).map_or(0, |l| (l.area() / shore::M2_PER_BATHER) as u32);
            if let Some(s) = self.shore.beaches.get_mut(&b) {
                s.access = reached;
                s.capacity = cap.max(4);
            }
        }
    }

    /// Does a breadth-first walk from `starts` reach a node that belongs to
    /// no lot (a pavement, crossing or path), within `limit` nodes?
    fn walk_reaches_street(&self, starts: &[u32], limit: usize) -> bool {
        let g = &self.walk;
        let mut seen: std::collections::HashSet<u32> = starts.iter().copied().collect();
        let mut queue: std::collections::VecDeque<u32> = starts.iter().copied().collect();
        while let Some(n) = queue.pop_front() {
            if g.nodes[n as usize].lot.is_none() {
                return true;
            }
            if seen.len() > limit {
                break;
            }
            for e in &g.adj[n as usize] {
                if seen.insert(e.to) {
                    queue.push_back(e.to);
                }
            }
        }
        false
    }

    /// The beaches as they stand: one per park building on a beach lot
    /// (named when it first opens as one); gone ones dropped.
    fn refresh_beach_list(&mut self) {
        // A beach behind other ground (reached, if at all, through a park)
        // is never developed like a lot on a street: the city lays it out.
        let bare: Vec<LotId> = self
            .lots
            .iter()
            .filter(|(id, l)| l.shore.beach && l.land_use == LandUse::Park && l.building.is_none() && l.landlocked && !self.under_facility(l.center) && !self.services.is_rubble(*id))
            .map(|(id, _)| id)
            .collect();
        for lot in bare {
            let c = self.lots[lot].center;
            let seed = (c.x.round() as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (c.y.round() as i64 as u64) ^ 0xBEAC_4000;
            let tier = Tier::for_land_value(self.land_value.at(c));
            let value = self.lot_value(lot);
            let b = Building::new(lot, LandUse::Park, 0, tier, seed, shore::beach_name(seed), self.clock.tick, value);
            let id = self.buildings.insert(b);
            self.lots[lot].building = Some(id);
            self.events.push(SimEvent::BuildingBuilt(id));
        }
        let now: Vec<(BuildingId, LotId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.land_use == LandUse::Park && b.operating())
            .filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| l.shore.beach))
            .map(|(id, b)| (id, b.lot))
            .collect();
        let keep: std::collections::HashSet<BuildingId> = now.iter().map(|x| x.0).collect();
        self.shore.beaches.retain(|b, _| keep.contains(b));
        for (b, _) in now {
            if self.shore.beaches.contains_key(&b) {
                continue;
            }
            let name = shore::beach_name(self.buildings[b].seed);
            self.buildings[b].name = name.clone();
            self.shore.beaches.insert(b, BeachState { name, open: true, ..Default::default() });
            self.shore.access_version = 0;
            self.events.push(SimEvent::BuildingChanged(b));
        }
    }

    /// On the hour: the beach list, access, contamination (a fouled beach
    /// closes with a sign, and opens when it clears), the weather's appeal
    /// and the crowd; the private jetties and their loads.
    pub(super) fn hourly_shore(&mut self) {
        self.refresh_beach_list();
        if !self.shore.beaches.is_empty() {
            self.refresh_beach_access();
            self.beach_crowds();
        }
        self.build_jetties();
        self.land_jetty_loads();
    }

    /// The crowd on every beach this hour: the residents within reach who
    /// would go in this weather at this hour (each to their nearest usable
    /// beach), the visitors from out of town, and whoever of the sim's own
    /// people is on the sand.
    fn beach_crowds(&mut self) {
        let appeal = self.beach_appeal_now();
        // (building, centre, lot, area)
        let mut list: Vec<(BuildingId, Vec2, LotId, f32)> = Vec::new();
        let ids: Vec<BuildingId> = self.shore.beaches.keys().copied().collect();
        for b in &ids {
            let Some(lot_id) = self.buildings.get(*b).map(|x| x.lot) else { continue };
            let Some(lot) = self.lots.get(lot_id) else { continue };
            let worst = self.beach_contamination(lot);
            let s = self.shore.beaches.get_mut(b).unwrap();
            s.contamination = worst;
            if s.open && worst >= shore::BEACH_CLOSE_AT {
                s.open = false;
            } else if !s.open && worst < shore::BEACH_REOPEN_AT {
                s.open = true;
            }
            s.appeal = appeal;
            list.push((*b, lot.center, lot_id, lot.area()));
        }
        // Residents to a usable beach within reach: each picks one by its
        // sand and how near it is (a hash of who they are and the day, so
        // neighbours spread over the beaches along a shore).
        let usable: Vec<(BuildingId, Vec2, f32)> = list.iter().filter(|(b, ..)| self.shore.beaches[b].usable()).map(|(b, c, _, a)| (*b, *c, *a)).collect();
        let mut catchment: HashMap<BuildingId, u32> = HashMap::new();
        if !usable.is_empty() {
            let day = self.clock.day();
            for c in self.citizens.values() {
                let Some(home) = c.home.and_then(|h| self.building_lot(h)).map(|l| l.center) else { continue };
                let weights: Vec<(BuildingId, f32)> = usable.iter().map(|(b, p, a)| (*b, p.distance(home), *a)).filter(|(_, d, _)| *d <= shore::BEACH_CATCHMENT).map(|(b, d, a)| (b, a.sqrt() / (1.0 + d / 300.0))).collect();
                let total: f32 = weights.iter().map(|w| w.1).sum();
                if total <= 0.0 {
                    continue;
                }
                let mut r = crate::pedestrians::hash01(c.seed, day, 0xBEAC) * total;
                let mut pick = weights[weights.len() - 1].0;
                for (b, w) in &weights {
                    if r < *w {
                        pick = *b;
                        break;
                    }
                    r -= w;
                }
                *catchment.entry(pick).or_default() += 1;
            }
        }
        // The sim's own people there now.
        let mut here: HashMap<BuildingId, u32> = HashMap::new();
        for c in self.citizens.values() {
            let at = match &c.activity {
                Activity::Sitting { park, .. } => Some(*park),
                Activity::Visiting(b) => Some(*b),
                _ => None,
            };
            if let Some(b) = at.filter(|b| self.shore.beaches.contains_key(b)) {
                *here.entry(b).or_default() += 1;
            }
        }
        let tourists: Vec<Vec2> = self.tourism.tourists.values().filter(|t| t.is_visible()).map(|t| t.pos).collect();
        let in_town = self.tourism.tourists.len() as f32;
        let n_usable = usable.len().max(1) as f32;
        for (b, _, lot_id, area) in list {
            let lot_poly = self.lots[lot_id].polygon.clone();
            let tourists_here = tourists.iter().filter(|p| point_in_polygon(**p, &lot_poly)).count() as u32;
            let s = self.shore.beaches.get_mut(&b).unwrap();
            s.catchment = catchment.get(&b).copied().unwrap_or(0);
            s.people_here = here.get(&b).copied().unwrap_or(0) + tourists_here;
            s.note = if !s.access {
                Some("no access: connect a path or a road, or a park next to it".to_string())
            } else if !s.open {
                Some(format!("closed: the water is fouled by industry ({:.0}% contaminated)", s.contamination * 100.0))
            } else if appeal < 0.02 {
                Some(if self.weather.spell == crate::weather::Spell::Storm {
                    "empty: a storm".to_string()
                } else if self.weather.temp_c < 12.0 {
                    format!("empty: too cold ({:.0} °C)", self.weather.temp_c)
                } else {
                    "empty at this hour".to_string()
                })
            } else {
                None
            };
            if !s.usable() {
                s.visitors = 0;
                s.swimmers = 0;
                s.draw = 0.0;
                continue;
            }
            let locals = s.catchment as f32 * shore::PEAK_SHARE * appeal;
            let visitors = in_town * 0.2 * appeal / n_usable;
            // A crowd past the comfortable one thins as people give up.
            let raw = locals + visitors;
            let cap = s.capacity.max(4) as f32;
            let crowd = if raw > cap { cap + (raw - cap) * 0.5 } else { raw };
            s.draw = crowd;
            let total = (crowd.round() as u32).max(s.people_here).min((area / 4.0) as u32);
            s.visitors = total;
            let warm = ((self.weather.temp_c - 18.0) / 14.0).clamp(0.0, 1.0);
            s.swimmers = (total as f32 * (0.15 + 0.35 * warm)).round() as u32;
            s.visits_today += total;
            s.peak_today = s.peak_today.max(total);
        }
        // Redraw a beach whose crowd, sign or lights changed.
        let night = !(7..20).contains(&self.clock.hour());
        let mut changed = Vec::new();
        for (b, s) in self.shore.beaches.iter_mut() {
            let key = (s.visitors / 4).min(60) | ((s.swimmers / 4).min(60) << 8) | (u32::from(!s.open) << 16) | (u32::from(night) << 17) | (1 << 20);
            if key != s.shown {
                s.shown = key;
                changed.push(*b);
            }
        }
        for b in changed {
            self.events.push(SimEvent::BuildingChanged(b));
        }
    }

    /// Midnight: the day's beach tallies roll, and households near an
    /// open beach that people used today are a little happier.
    pub(super) fn daily_shore(&mut self) {
        let mut good: Vec<Vec2> = Vec::new();
        for (b, s) in self.shore.beaches.iter_mut() {
            if s.usable() && s.visits_today > 0 {
                if let Some(c) = self.buildings.get(*b).and_then(|x| self.lots.get(x.lot)).map(|l| l.center) {
                    good.push(c);
                }
            }
            s.visits_yesterday = s.visits_today;
            s.visits_today = 0;
            s.peak_today = 0;
        }
        if good.is_empty() {
            return;
        }
        let homes: Vec<(CitizenId, Vec2)> = self.citizens.iter().filter_map(|(id, c)| c.home.and_then(|h| self.building_lot(h)).map(|l| (id, l.center))).collect();
        for (id, at) in homes {
            if good.iter().any(|p| p.distance(at) <= shore::BEACH_HAPPY_REACH) {
                let c = &mut self.citizens[id];
                c.mood = (c.mood + 0.02).min(1.0);
            }
        }
    }

    /// A spot on a beach for a visitor (a towel spot by their seed), for a
    /// sight at `at` on a beach's boardwalk; `at` itself otherwise.
    pub(super) fn beach_spot(&self, at: Vec2, seed: u64) -> Vec2 {
        let lot = self.shore.beaches.keys().filter_map(|b| self.buildings.get(*b)).filter_map(|b| self.lots.get(b.lot)).filter(|l| l.center.distance(at) < BEACH_SPOT_REACH).min_by(|a, b| a.center.distance(at).total_cmp(&b.center.distance(at)));
        let Some(spots) = lot.and_then(|l| l.park.as_ref()).map(|p| &p.benches).filter(|b| !b.is_empty()) else { return at };
        spots[(seed % spots.len() as u64) as usize].0
    }

    /// What open beaches and waterfront lots do for the land: a beach lifts
    /// the streets around it (less while it is shut), a lot on the water
    /// carries a premium by its frontage.
    pub(super) fn splat_shore(&self, b: &mut LandValueBuilder) {
        for l in self.lots.values() {
            if l.shore.is_waterfront() && !l.shore.beach {
                let (r, w) = shore::WATERFRONT_LIFT;
                b.splat(l.center, r, w * (l.shore.frontage / shore::FULL_FRONTAGE).min(1.0));
            }
        }
        for (bid, s) in &self.shore.beaches {
            let Some(c) = self.buildings.get(*bid).and_then(|x| self.lots.get(x.lot)).map(|l| l.center) else { continue };
            let (r, w) = shore::BEACH_LIFT;
            let lift = if !s.access { w * 0.4 } else if s.open { w } else { w * 0.25 };
            b.splat(c, r, lift);
        }
    }

    // ── private jetties ────────────────────────────────────────────────

    /// The jetty a business on the water builds for itself, if any.
    pub fn jetty_kind_for(kind: BusinessKind) -> Option<JettyKind> {
        use BusinessKind as B;
        Some(match kind {
            B::Fishery | B::FishPlant | B::FishGutting | B::FishStickFactory | B::FishMarket => JettyKind::Fishing,
            B::ContainerYard | B::Refinery | B::Smelter | B::ChemicalWorks | B::Mill | B::Assembly | B::Fab | B::CargoHub => JettyKind::Cargo,
            B::PierRestaurant => JettyKind::Pier,
            B::BoatRental => JettyKind::Pontoon,
            _ => return None,
        })
    }

    /// Businesses on the water that have none yet build their jetty (the
    /// works pay for theirs; a restaurant's pier and a rental's pontoon
    /// come with the building). Jetties of businesses gone or changed go.
    fn build_jetties(&mut self) {
        let tick = self.clock.tick;
        let stale: Vec<BuildingId> = self
            .shore
            .jetties
            .iter()
            .filter(|(b, j)| self.buildings.get(**b).and_then(|x| x.business.as_ref()).and_then(|z| Self::jetty_kind_for(z.kind)) != Some(j.kind))
            .map(|(b, _)| *b)
            .collect();
        for b in stale {
            self.shore.jetties.remove(&b);
            self.events.push(SimEvent::BuildingChanged(b));
        }
        if self.landscape.is_flat() {
            return;
        }
        let candidates: Vec<(BuildingId, JettyKind)> = self
            .buildings
            .iter()
            .filter(|(id, b)| b.operating() && !self.shore.jetties.contains_key(id))
            .filter_map(|(id, b)| {
                let z = b.business.as_ref().filter(|z| !z.closed)?;
                let k = Self::jetty_kind_for(z.kind)?;
                self.lots.get(b.lot).filter(|l| l.shore.is_waterfront())?;
                Some((id, k))
            })
            .collect();
        for (b, kind) in candidates {
            let paid = matches!(kind, JettyKind::Fishing | JettyKind::Cargo);
            if paid && self.buildings[b].business.as_ref().is_none_or(|z| z.money < shore::JETTY_COST * 1.2) {
                continue;
            }
            let Some(lot) = self.lots.get(self.buildings[b].lot) else { continue };
            let Ok(mut j) = shore::plan_jetty(lot, &self.landscape, kind) else { continue };
            j.built = tick;
            if paid {
                if let Some(z) = self.buildings[b].business.as_mut() {
                    z.money -= shore::JETTY_COST;
                    z.spend_today += shore::JETTY_COST;
                }
            }
            self.shore.jetties.insert(b, j);
            self.events.push(SimEvent::BuildingChanged(b));
        }
    }

    /// The working cargo quay of a business, whose coasters can reach the
    /// world beyond the map.
    fn cargo_jetty(&self, b: BuildingId) -> Option<&Jetty> {
        self.shore.jetties.get(&b).filter(|j| j.kind == JettyKind::Cargo && self.buildings.get(b).is_some_and(|x| x.operating()))
    }

    /// Import through the buyer's own quay: the sea price, landed by a
    /// coaster a few hours on. False when it has none.
    pub(super) fn import_by_jetty(&mut self, to: BuildingId, cargo: Cargo) -> bool {
        if self.cargo_jetty(to).is_none() {
            return false;
        }
        let refund = cargo.price * (1.0 - crate::ports::SHIP_IMPORT_FACTOR);
        if let Some(bz) = self.buildings.get_mut(to).and_then(|b| b.business.as_mut()) {
            bz.money += refund;
            bz.spend_today -= refund;
        }
        self.economy.market.imports_today -= refund;
        let due = self.clock.tick + shore::JETTY_LEAD_HOURS * 60;
        self.shore.inbound.push((to, Cargo { from: to, price: cargo.price * crate::ports::SHIP_IMPORT_FACTOR, ..cargo }, due));
        true
    }

    /// Loads due at a private quay are landed; a quay that went (its
    /// business closed or pulled down) sends them on by road.
    fn land_jetty_loads(&mut self) {
        if self.shore.inbound.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.shore.inbound).into_iter().partition(|(_, _, t)| *t <= tick);
        self.shore.inbound = wait;
        for (b, cargo, _) in due {
            if self.cargo_jetty(b).is_some() {
                if let Some(j) = self.shore.jetties.get_mut(&b) {
                    j.loads += 1;
                    j.last_load = tick;
                }
                self.deliver(cargo);
            } else if self.buildings.contains_key(b) {
                self.dispatch_truck(super::market::TruckOrigin::CityEdge, cargo, 6);
            }
        }
    }
}

impl World {
    /// [waterfront] The middles of the public beaches people can use now.
    pub(super) fn usable_beach_centres(&self) -> Vec<Vec2> {
        self.shore
            .beaches
            .iter()
            .filter(|(_, s)| s.usable())
            .filter_map(|(b, _)| self.buildings.get(*b).and_then(|x| self.lots.get(x.lot)).map(|l| l.center))
            .collect()
    }
}
