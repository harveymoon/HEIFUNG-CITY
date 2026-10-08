//! [land] The land market on the `World`: the pressure grid sampled hourly,
//! sale prices and owners, developers buying out low buildings where the
//! city presses for more, firms opening branches or buying the plots next
//! door, families merging two plots for a McMansion, and the super-elite:
//! rare billionaires with mansions, NIMBY pull and escorted convoys.
//!
//! Costs: one pass over citizens and cars an hour, one over buildings and
//! lots a day, the convoys (a handful of cars) every tick. Draws come from
//! hash-seeded generators (`erng`), never the world's own stream, so the
//! rest of the simulation rolls the same dice as before.

use super::*;
use crate::estate::*;
use std::borrow::Cow;
use crate::vehicle::VehicleModel;

const ESTATE_SALT: u64 = 0x5E57_A7E0_1A4D;

/// Fictional property developers.
const DEVELOPERS: &[&str] = &["Kessler Land", "Halcyon Developments", "Argent Holdings", "Meridian Estates", "Nocturne Property", "Zenith Build", "Fulcrum Realty"];

impl World {
    /// A generator for one draw site, keyed by the tick: deterministic and
    /// independent of the world's own stream.
    pub(super) fn erng(&self, salt: u64) -> SimRng {
        crate::rng_for(self.config.seed ^ ESTATE_SALT ^ salt.wrapping_mul(0x51_7C_C1_B7), self.clock.tick)
    }

    // ── pressure ───────────────────────────────────────────────────────────

    /// Every hour: people present and moving in each cell, and the share of
    /// cars standing, folded into the running averages.
    pub(super) fn hourly_pressure(&mut self) {
        if !self.estate.pressure.ready() {
            self.estate.pressure = Pressure::new(self.config.half_extent + 200.0);
        }
        let p = &self.estate.pressure;
        let cells = p.value.len();
        let mut present = vec![0.0f32; cells];
        let mut cars = vec![0.0f32; cells];
        let mut stalled = vec![0.0f32; cells];
        for c in self.citizens.values() {
            if let Some(i) = p.index(c.pos) {
                present[i] += 1.0;
                // Walking or cycling through counts again: movement.
                if matches!(c.activity, Activity::Traveling { .. }) {
                    present[i] += 1.0;
                }
            }
        }
        for car in self.traffic.cars.values() {
            if let Some(i) = p.index(car.pos) {
                present[i] += 1.0;
                cars[i] += 1.0;
                if car.v < 0.5 {
                    stalled[i] += 1.0;
                }
            }
        }
        let share: Vec<f32> = cars.iter().zip(&stalled).map(|(c, s)| if *c > 0.0 { s / c } else { 0.0 }).collect();
        self.estate.pressure.sample(&present, &share);
    }

    /// Daily: how well each cell moves people. Bus stops within 300 m and a
    /// station or metro entrance within 600 m raise it; cars standing in
    /// traffic lower it. Building height follows (`Pressure::floors_at`).
    pub(super) fn daily_mobility(&mut self) {
        if !self.estate.pressure.ready() {
            return;
        }
        let stops: Vec<Vec2> = self.transit.routes.values().flat_map(|r| r.stops.iter().map(|s| s.pos)).collect();
        let stations: Vec<Vec2> = self
            .buildings
            .values()
            .filter(|b| b.operating() && (b.metro || b.rail.as_ref().is_some_and(|l| l.kind != RailFacility::FreightYard)))
            .filter_map(|b| self.lots.get(b.lot).map(|l| l.center))
            .collect();
        let p = &mut self.estate.pressure;
        for i in 0..p.value.len() {
            let c = p.cell_center(i);
            let bus = (stops.iter().filter(|s| s.distance(c) < 300.0).count() as f32 / 3.0).min(1.0);
            let rail = if stations.iter().any(|s| s.distance(c) < 600.0) { 1.0 } else { 0.0 };
            let transit = 0.5 * bus + 0.5 * rail;
            p.mobility[i] = (0.8 + 0.5 * transit - 0.6 * p.congestion[i]).clamp(0.5, 1.5);
        }
    }

    /// Floors the pressure on a lot justifies (two where it is quiet), held
    /// to `NIMBY_HEIGHT_CAP` inside a billionaire's NIMBY radius.
    /// Does pressure cap heights yet? Measured at least once, in a town of
    /// `PRESSURE_MIN_POP` people.
    pub fn pressure_active(&self) -> bool {
        let p = &self.estate.pressure;
        p.samples > 0 && (p.always || self.citizens.len() >= PRESSURE_MIN_POP)
    }

    pub fn pressure_floors(&self, lot_id: LotId) -> u8 {
        let Some(lot) = self.lots.get(lot_id) else { return PRESSURE_MAX_FLOORS };
        let f = self.estate.pressure.floors_at(lot.center);
        if self.estate.in_nimby(lot.center) {
            f.min(NIMBY_HEIGHT_CAP)
        } else {
            f
        }
    }

    // ── prices and owners ──────────────────────────────────────────────────

    /// What it would take to buy a building and its plots: the land at its
    /// value, the structure by size, tier and condition, and a business's
    /// goodwill (a year of its recent profit). Every owner has one.
    pub fn sale_price(&self, bid: BuildingId) -> f64 {
        let Some(b) = self.buildings.get(bid) else { return 0.0 };
        let area: f32 = b.plot.lots(b.lot).iter().filter_map(|l| self.lots.get(*l)).map(|l| l.area()).sum();
        let value = b.land_value.max(0.0);
        let land = area as f64 * (25.0 + 400.0 * value as f64);
        let (homes, jobs) = b.planned_capacity();
        let per_unit = match b.tier {
            Tier::Low => 15_000.0,
            Tier::Mid => 40_000.0,
            Tier::High => 120_000.0,
        };
        let structure = (homes as f64 + jobs as f64) * per_unit * (0.4 + 0.6 * b.condition as f64);
        let goodwill = b.business.as_ref().map_or(0.0, |z| {
            let n = z.books.len().max(1) as f64;
            let profit: f64 = z.books.iter().map(|d| (d.revenue - d.spend) as f64).sum::<f64>() / n;
            (profit * 365.0).max(0.0)
        });
        let grand = match b.plot.form {
            Some(Form::Mansion) => 3.0,
            Some(Form::McMansion) => 1.6,
            _ => 1.0,
        };
        ((land + structure + goodwill) * grand).round()
    }

    /// What `bid` would be worth rebuilt at `tier` and `floors`: its land
    /// and the new structure, before any business makes it its own.
    fn value_of_new(&self, bid: BuildingId, tier: Tier, floors: u8) -> f64 {
        let Some(b) = self.buildings.get(bid) else { return 0.0 };
        let mut n = b.clone();
        n.tier = tier;
        n.floors = floors;
        n.construction = None;
        n.demolition = None;
        n.condition = 1.0;
        n.business = None;
        n.recompute_capacity();
        let area: f32 = b.plot.lots(b.lot).iter().filter_map(|l| self.lots.get(*l)).map(|l| l.area()).sum();
        let land = area as f64 * (25.0 + 400.0 * b.land_value.max(0.0) as f64);
        let per_unit = match tier {
            Tier::Low => 15_000.0,
            Tier::Mid => 40_000.0,
            Tier::High => 120_000.0,
        };
        land + (n.homes as f64 + n.jobs as f64) * per_unit
    }

    pub fn owner_of(&self, bid: BuildingId) -> Option<(OwnerId, &Owner)> {
        let oid = self.buildings.get(bid)?.plot.owner?;
        self.estate.owners.get(oid).map(|o| (oid, o))
    }

    /// Everything the owner of `bid` owns (`bid` itself first), for the
    /// selection highlight; just `bid` for an independent owner. A
    /// billionaire's other homes and businesses are among them.
    pub fn owned_with(&self, bid: BuildingId) -> Vec<BuildingId> {
        let mut out = vec![bid];
        if let Some((_, o)) = self.owner_of(bid) {
            out.extend(o.properties.iter().copied().filter(|p| *p != bid && self.buildings.contains_key(*p)));
        }
        out
    }

    /// The ground a building stands on as one lot: its own, or for a
    /// building over merged plots their combined outline
    /// (`estate::merged_lot`). The renderer draws on this.
    pub fn footprint(&self, bid: BuildingId) -> Option<Cow<'_, Lot>> {
        let b = self.buildings.get(bid)?;
        let lot = self.lots.get(b.lot)?;
        if b.plot.extra_lots.is_empty() {
            return Some(Cow::Borrowed(lot));
        }
        let others: Vec<&Lot> = b.plot.extra_lots.iter().filter_map(|l| self.lots.get(*l)).collect();
        // [supertall] A tower's assembled site may be a little less compact.
        let slack = if b.plot.form == Some(Form::Tower) { crate::towers::TOWER_HULL_SLACK } else { MERGE_HULL_SLACK };
        Some(match merged_lot_within(lot, &others, slack) {
            Some(m) => Cow::Owned(m),
            None => Cow::Borrowed(lot),
        })
    }

    /// The social class of a citizen: the income classes, and the
    /// super-elite above them.
    pub fn class_of(&self, cid: CitizenId) -> Class {
        if self.estate.is_elite(cid) {
            return Class::Elite;
        }
        match self.citizens.get(cid).map(|c| c.class()) {
            Some(Tier::High) => Class::High,
            Some(Tier::Mid) => Class::Middle,
            _ => Class::Low,
        }
    }

    pub(super) fn new_owner(&mut self, name: String, kind: OwnerKind) -> OwnerId {
        let day = self.clock.day();
        self.estate.owners.insert(Owner { name, kind, properties: Vec::new(), spent: 0.0, since_day: day as u32 })
    }

    /// `owner` buys `bid` (or takes it on as built): off its old owner's
    /// books, onto the new one's.
    pub(super) fn give_to(&mut self, bid: BuildingId, owner: OwnerId, price: f64) {
        let Some(b) = self.buildings.get_mut(bid) else { return };
        let old = b.plot.owner.replace(owner);
        if let Some(o) = old.filter(|o| *o != owner).and_then(|o| self.estate.owners.get_mut(o)) {
            o.properties.retain(|p| *p != bid);
        }
        if let Some(o) = self.estate.owners.get_mut(owner) {
            if !o.properties.contains(&bid) {
                o.properties.push(bid);
            }
            o.spent += price;
        }
    }

    /// The owner record that is `cid`'s own: their billionaire record, a
    /// person record, or the household's (a McMansion), made on first use.
    pub(super) fn owner_for_citizen(&mut self, cid: CitizenId) -> Option<OwnerId> {
        let c = self.citizens.get(cid)?;
        let hh = c.household;
        let name = c.name.clone();
        let found = self.estate.owners.iter().find(|(_, o)| o.kind.citizen() == Some(cid)).map(|(id, _)| id);
        if let Some(o) = found {
            return Some(o);
        }
        if let Some(o) = hh.and_then(|h| self.estate.owners.iter().find(|(_, o)| o.kind == OwnerKind::Household { household: h }).map(|(id, _)| id)) {
            return Some(o);
        }
        Some(self.new_owner(name, OwnerKind::Person { citizen: cid }))
    }

    /// `cid` owns the business at `bid` (a founder): the owner record is
    /// the source of truth; `Business::owner` mirrors it for older code
    /// and saves.
    pub fn set_business_owner(&mut self, bid: BuildingId, cid: CitizenId) {
        let Some(o) = self.owner_for_citizen(cid) else { return };
        if let Some(z) = self.buildings.get_mut(bid).and_then(|b| b.business.as_mut()) {
            z.owner = Some(cid);
        }
        self.give_to(bid, o, 0.0);
    }

    /// Every building `cid` owns: through their owner records (a
    /// billionaire's homes and firms, a family's McMansion, a founder's
    /// firm), and any business that still names them only on its own
    /// books. Sorted, each once.
    pub fn properties_of(&self, cid: CitizenId) -> Vec<BuildingId> {
        let hh = self.citizens.get(cid).and_then(|c| c.household);
        let mut out: Vec<BuildingId> = self
            .estate
            .owners
            .values()
            .filter(|o| o.kind.citizen() == Some(cid) || hh.is_some_and(|h| o.kind == OwnerKind::Household { household: h }))
            .flat_map(|o| o.properties.iter().copied())
            .filter(|b| self.buildings.contains_key(*b))
            .collect();
        out.extend(self.buildings.iter().filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.owner == Some(cid))).map(|(id, _)| id));
        out.sort();
        out.dedup();
        out
    }

    /// The owner record of a firm, made on its first branch or merge.
    fn firm_of(&mut self, bid: BuildingId) -> Option<OwnerId> {
        let b = self.buildings.get(bid)?;
        if let Some(o) = b.plot.owner.filter(|o| self.estate.owners.contains_key(*o)) {
            return Some(o);
        }
        let kind = b.business.as_ref()?.kind;
        let name = b.name.clone();
        let o = self.new_owner(name, OwnerKind::Firm { kind });
        self.give_to(bid, o, 0.0);
        Some(o)
    }

    /// A developer to buy with: one of up to four, made as needed.
    fn developer(&mut self, rng: &mut SimRng) -> OwnerId {
        let devs: Vec<OwnerId> = self.estate.owners.iter().filter(|(_, o)| o.kind == OwnerKind::Developer).map(|(id, _)| id).collect();
        if devs.len() >= 4 || (!devs.is_empty() && rng.random_bool(0.6)) {
            return devs[rng.random_range(0..devs.len())];
        }
        let taken: Vec<String> = self.estate.owners.values().map(|o| o.name.clone()).collect();
        let name = DEVELOPERS.iter().find(|n| !taken.iter().any(|t| t == *n)).map(|s| s.to_string()).unwrap_or_else(|| format!("Developer {}", devs.len() + 1));
        self.new_owner(name, OwnerKind::Developer)
    }

    /// Can the market buy this building? Built and standing, ordinary (not
    /// the city's, a car park, a taxi yard or cargo hub), not in a frozen
    /// historic district, not a home bought outright, not a billionaire's.
    pub(super) fn buyable(&self, bid: BuildingId) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        if !b.operating() || b.is_special() || b.is_parking() || b.land_use == LandUse::Park || b.land_use == LandUse::Civic {
            return false;
        }
        if b.business.as_ref().is_some_and(|z| matches!(z.kind, BusinessKind::TaxiCompany | BusinessKind::CargoHub)) {
            return false;
        }
        if b.plot.form.is_some_and(|f| f.owner_occupied()) || !b.plot.pending_lots.is_empty() {
            return false;
        }
        // [supertall] A league tower is nobody's teardown.
        if b.plot.tower.is_some() {
            return false;
        }
        if let Some((_, o)) = self.owner_of(bid) {
            // [megacorp] A corp's or a university's holdings are not for sale.
            if matches!(o.kind, OwnerKind::Elite { .. } | OwnerKind::Household { .. } | OwnerKind::MegaCorp { .. } | OwnerKind::University { .. }) {
                return false;
            }
        }
        !self.district_frozen(b.lot)
    }

    // ── the day ────────────────────────────────────────────────────────────

    /// Once a day, after the daily update: mobility, owners' books, buyouts
    /// where the city presses for more, firms growing, families and
    /// billionaires buying, NIMBY fading.
    pub(super) fn daily_estate(&mut self) {
        crate::prof_scope!("daily_estate");
        self.estate.stats.buyouts_today = 0;
        self.prune_estate();
        self.daily_mobility();
        self.densify();
        self.expand_firms();
        self.build_mcmansions();
        // [rich] The well-off buy a business or a second home.
        self.invest_rich();
        self.daily_elites();
        self.daily_nimby();
    }

    /// Drop what no longer exists: owners' sold or demolished properties,
    /// plots pointing at buildings that do not span them, reservations of
    /// buildings that are gone.
    pub(super) fn prune_estate(&mut self) {
        let buildings = &self.buildings;
        for o in self.estate.owners.values_mut() {
            o.properties.retain(|p| buildings.contains_key(*p));
        }
        // Owners of nothing (and nobody's billionaire) go.
        // Founders named only on a business's books (older saves) get
        // their owner record.
        let named: Vec<(BuildingId, CitizenId)> = self.buildings.iter().filter(|(_, b)| b.plot.owner.is_none()).filter_map(|(id, b)| b.business.as_ref().and_then(|z| z.owner).filter(|c| self.citizens.contains_key(*c)).map(|c| (id, c))).collect();
        for (b, c) in named {
            self.set_business_owner(b, c);
        }
        let elites: Vec<OwnerId> = self.estate.elites.iter().map(|e| e.owner).collect();
        self.estate.owners.retain(|id, o| !o.properties.is_empty() || elites.contains(&id));
        for b in self.buildings.values_mut() {
            if b.plot.owner.is_some_and(|o| !self.estate.owners.contains_key(o)) {
                b.plot.owner = None;
            }
        }
        let lots = &self.lots;
        let buildings = &self.buildings;
        self.estate.reserved.retain(|l, b| lots.contains_key(*l) && buildings.contains_key(*b));
        // Every lot naming a building must be its primary or one of its plots.
        let mut stray: Vec<LotId> = Vec::new();
        for (lid, l) in self.lots.iter() {
            let Some(bid) = l.building else { continue };
            match self.buildings.get(bid) {
                Some(b) if b.lot == lid || b.plot.extra_lots.contains(&lid) => {}
                _ => stray.push(lid),
            }
        }
        for lid in stray {
            self.lots[lid].building = None;
        }
        let lots = &self.lots;
        for (bid, b) in self.buildings.iter_mut() {
            b.plot.extra_lots.retain(|l| lots.get(*l).is_some_and(|x| x.building == Some(bid)));
        }
        let cars = &self.traffic.cars;
        let citizens = &self.citizens;
        self.estate.elites.retain(|e| citizens.contains_key(e.citizen));
        for c in &mut self.estate.convoys {
            c.escorts.retain(|e| cars.contains_key(*e));
            c.police = c.police.filter(|p| cars.contains_key(*p));
        }
        let tick = self.clock.tick;
        self.estate.convoys.retain(|c| cars.contains_key(c.boss) || !c.escorts.is_empty() || c.police.is_some() || c.stop.is_some_and(|(_, t)| t > tick));
        // [rich] Garages of people who left are sold off.
        self.prune_spare_cars();
    }

    /// A building is gone (removed outright or cleared after demolition):
    /// its extra plots are free again, it leaves its owner's books, and any
    /// plot it stood on that someone bought is handed over.
    pub(super) fn estate_building_gone(&mut self, bid: BuildingId, plot: &Plot, lot: LotId) {
        for l in plot.extra_lots.iter().chain(plot.pending_lots.iter()) {
            if self.lots.get(*l).is_some_and(|x| x.building == Some(bid)) {
                self.lots[*l].building = None;
            }
            if self.estate.reserved.get(l) == Some(&bid) {
                self.estate.reserved.remove(l);
            }
        }
        if let Some(o) = plot.owner.and_then(|o| self.estate.owners.get_mut(o)) {
            o.properties.retain(|p| *p != bid);
        }
        self.estate.nimby.retain(|n| n.mansion != bid);
        for e in &mut self.estate.elites {
            if e.mansion == Some(bid) {
                e.mansion = None;
            }
        }
        self.claim_reserved(lot);
        for l in plot.extra_lots.clone() {
            self.claim_reserved(l);
        }
    }

    /// A plot someone bought is clear: it becomes part of their building.
    pub(super) fn claim_reserved(&mut self, lot: LotId) {
        let Some(&bid) = self.estate.reserved.get(&lot) else { return };
        if self.lots.get(lot).is_none_or(|l| l.building.is_some()) {
            if self.lots.get(lot).is_none() {
                self.estate.reserved.remove(&lot);
            }
            return;
        }
        self.estate.reserved.remove(&lot);
        let Some(b) = self.buildings.get_mut(bid) else { return };
        b.plot.pending_lots.retain(|l| *l != lot);
        if !b.plot.extra_lots.contains(&lot) && b.lot != lot {
            b.plot.extra_lots.push(lot);
        }
        b.recompute_capacity();
        self.lots[lot].building = Some(bid);
        self.events.push(SimEvent::BuildingChanged(bid));
        self.events.push(SimEvent::LotsChanged);
    }

    /// Buy `plots` for `bid`: vacant ones join at once, built ones are
    /// pulled down first (`memory` for those who must leave) and join when
    /// the rubble is gone.
    pub(super) fn take_plots(&mut self, bid: BuildingId, plots: &[LotId], memory: &str) {
        for &l in plots {
            let Some(lot) = self.lots.get(l) else { continue };
            match lot.building {
                Some(b) if b == bid => {}
                None => {
                    self.estate.reserved.insert(l, bid);
                    self.claim_reserved(l);
                }
                Some(other) => {
                    // [relocate] A business next door looks for a new place.
                    let why = format!("{}'s expansion", self.buildings.get(bid).map_or(String::new(), |b| b.name.clone()));
                    self.pack_business(other, &why);
                    // A park standing on rezoned ground just goes.
                    self.estate.reserved.insert(l, bid);
                    if let Some(b) = self.buildings.get_mut(bid) {
                        b.plot.pending_lots.push(l);
                    }
                    self.demolish_with(other, None, Some(memory));
                    // Cleared at once (a car park, a site at groundworks).
                    self.claim_reserved(l);
                }
            }
        }
    }

    /// Plots touching any of `ours` that could be bought for a building of
    /// `lu`: same land use, not under a facility or already promised, empty
    /// (landlocked backland included) or holding a building the market may
    /// buy that is no taller than `max_floors`. Cheapest first, vacant first.
    fn plots_next_to(&self, ours: &[LotId], lu: LandUse, max_floors: u8, owner: Option<OwnerId>) -> Vec<(f64, LotId)> {
        let mine: Vec<&Lot> = ours.iter().filter_map(|l| self.lots.get(*l)).collect();
        let mut out: Vec<(f64, LotId)> = Vec::new();
        for (lid, l) in self.lots.iter() {
            if l.land_use != lu || ours.contains(&lid) || self.estate.reserved.contains_key(&lid) || self.under_facility(l.center) {
                continue;
            }
            if !mine.iter().any(|m| m.center.distance(l.center) < 120.0 && lots_touch(m, l)) {
                continue;
            }
            let cost = match l.building {
                None => 0.0,
                Some(b) => {
                    let Some(bld) = self.buildings.get(b) else { continue };
                    if !self.buyable(b) || bld.floors > max_floors || (owner.is_some() && bld.plot.owner == owner) || bld.plot.merged() {
                        continue;
                    }
                    self.sale_price(b)
                }
            };
            out.push((cost, lid));
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        out
    }

    // ── densification ──────────────────────────────────────────────────────

    /// Where the city presses for more than stands, developers buy the low
    /// buildings out, pull them down and build bigger: the most pressed
    /// first, a few a day. Residents move, a business closes (the new
    /// building opens as what the market wants).
    pub(super) fn densify(&mut self) {
        let mut rng = self.erng(1);
        if self.estate.pressure.samples < 24 {
            return;
        }
        // A few a day: a town rebuilds a street at a time, not a quarter.
        let max = (1 + self.buildings.len() / 400).min(4);
        let tick = self.clock.tick;
        let mut cands: Vec<(u8, BuildingId)> = Vec::new();
        for (id, b) in self.buildings.iter() {
            if !matches!(b.land_use, LandUse::Residential | LandUse::Commercial | LandUse::Entertainment) || b.plot.form.is_some() || b.plot.owner.is_some() {
                continue;
            }
            if tick.saturating_sub(b.built_tick) < 3 * 1440 || !self.buyable(id) {
                continue;
            }
            let pf = self.pressure_floors(b.lot);
            if pf >= b.floors.saturating_mul(2).max(b.floors + 3) {
                cands.push((pf - b.floors, id));
            }
        }
        cands.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut done = 0;
        for (gap, bid) in cands.into_iter().take(max * 3) {
            if done >= max {
                break;
            }
            let (lu, lot, old) = {
                let b = &self.buildings[bid];
                (b.land_use, b.lot, b.floors)
            };
            // Only while people want more of it here.
            let demand = self.economy.demand_for(lu);
            if demand < 0.2 {
                continue;
            }
            let chance = ((gap as f32 / 8.0).min(1.0) * (0.3 + demand)).clamp(0.05, 0.85);
            if rng.random::<f32>() >= chance {
                continue;
            }
            let cap = self.height_cap(lot);
            if cap < old + 2 {
                continue;
            }
            if self.buy_out_and_rebuild(bid, &mut rng) {
                done += 1;
            }
        }
    }

    /// A developer buys `bid` at its sale price, pulls it down and builds
    /// bigger at the land's tier. True if it went ahead.
    pub fn buy_out_and_rebuild(&mut self, bid: BuildingId, rng: &mut SimRng) -> bool {
        if !self.buyable(bid) {
            return false;
        }
        let price = self.sale_price(bid);
        let dev = self.developer(rng);
        let (lu, lot, old_floors, name, had_business) = {
            let b = &self.buildings[bid];
            (b.land_use, b.lot, b.floors, b.name.clone(), b.business.is_some())
        };
        let value = self.lot_value(lot);
        let mut tier = Tier::for_land_value(value);
        if lu == LandUse::Residential {
            tier = self.residential_tier(tier);
        }
        if let Some(l) = self.buildings[bid].look {
            tier = l.tier(tier);
        }
        let floors = self.pick_floors(lot, lu, tier).max(old_floors + 1).min(self.height_cap(lot).max(old_floors + 1));
        // A better use, or no sale: what the new building is worth (land
        // and structure, no goodwill yet) must clear the price by a fifth.
        // A thriving business's goodwill keeps it off the market.
        if self.value_of_new(bid, tier, floors) < price * 1.2 {
            return false;
        }
        let dev_name = self.estate.owners.get(dev).map(|o| o.name.clone()).unwrap_or_default();
        let memory = format!("{name} was sold to {dev_name} for ${:.0}k and is coming down for something bigger. We had to go.", price / 1000.0);
        // [relocate] The business looks for a new place.
        if had_business {
            self.pack_business(bid, &format!("{dev_name}'s rebuild"));
        }
        self.redevelop_with(bid, tier, floors, value, memory, false);
        if had_business {
            // The old business closes with the sale; the new building
            // opens as what the market wants now.
            let jobs = self.buildings[bid].planned_capacity().1;
            let kind = self.pick_business_kind(lu, jobs);
            self.buildings[bid].business = kind.map(|k| Business::new(k, jobs));
        }
        self.give_to(bid, dev, price);
        self.estate.stats.buyouts_total += 1;
        self.estate.stats.buyouts_today += 1;
        true
    }

    // ── firms ──────────────────────────────────────────────────────────────

    /// The "wants to expand" trigger: the business says so itself
    /// (`Business::wants_expand`, set by the economy's pressures: a gallery
    /// with enough donations), or, generically, it is open, has paid its
    /// way for its last three days with room to spare, holds cash for the
    /// move and is mostly staffed.
    pub fn wants_to_expand(&self, bid: BuildingId) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        let Some(z) = &b.business else { return false };
        if z.wants_expand && !z.closed && b.operating() {
            return true;
        }
        if z.closed || !b.operating() || b.is_derelict() || z.days_open < 6 || z.books.len() < 3 {
            return false;
        }
        let last = &z.books[z.books.len() - 3..];
        let paying = last.iter().all(|d| d.revenue > d.spend * 1.1 && d.revenue > 0.0);
        let staffed = b.jobs == 0 || b.staffing() >= 0.7;
        paying && staffed && z.money >= 400.0 + 60.0 * b.jobs as f32
    }

    /// Successful firms grow, two a day at most: branch kinds open a
    /// branch across the city, the rest buy the plot next door.
    pub(super) fn expand_firms(&mut self) {
        let mut rng = self.erng(2);
        let mut cands: Vec<(f32, BuildingId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.business.is_some() && !b.is_special() && b.plot.pending_lots.is_empty())
            .map(|(id, b)| (b.business.as_ref().map_or(0.0, |z| z.money), id))
            .collect();
        cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut done = 0;
        for (_, bid) in cands {
            if done >= 2 {
                break;
            }
            if !self.wants_to_expand(bid) || !rng.random_bool(0.2) {
                continue;
            }
            let kind = self.buildings[bid].business.as_ref().unwrap().kind;
            let grew = if branches(kind) { self.open_branch(bid).is_some() } else if expansion_form(kind).is_some() { self.expand_into_neighbours(bid) } else { false };
            if grew {
                done += 1;
            }
        }
    }

    /// A branch of the firm at `bid` on a vacant lot of its land use, as
    /// far as can be from the firm's other properties (at least 450 m), so
    /// the chain scatters across the city. The firm pays 1,500.
    pub fn open_branch(&mut self, bid: BuildingId) -> Option<BuildingId> {
        let (kind, lu) = {
            let b = self.buildings.get(bid)?;
            (b.business.as_ref()?.kind, b.land_use)
        };
        let owner = self.firm_of(bid)?;
        let mine: Vec<Vec2> = self.estate.owners[owner].properties.iter().filter_map(|p| self.building_lot(*p)).map(|l| l.center).collect();
        let cap = 1 + self.citizens.len() / 1500;
        if mine.len() > cap.min(6) {
            return None;
        }
        let taxi = kind == BusinessKind::TaxiCompany;
        let mut best: Option<(f32, LotId)> = None;
        for (lid, l) in self.lots.iter() {
            if l.building.is_some() || !l.usable() || self.estate.reserved.contains_key(&lid) || self.under_facility(l.center) {
                continue;
            }
            let ok_use = if taxi { matches!(l.land_use, LandUse::Commercial | LandUse::Industrial) && l.car_segment.is_some() && l.area() >= crate::taxi::YARD_MIN_AREA } else { l.land_use == lu };
            if !ok_use {
                continue;
            }
            let d = mine.iter().map(|m| m.distance(l.center)).fold(f32::MAX, f32::min);
            if d < 450.0 {
                continue;
            }
            if best.is_none_or(|(bd, bl)| d > bd || (d == bd && lid < bl)) {
                best = Some((d, lid));
            }
        }
        let (_, lot) = best?;
        let n = self.estate.owners[owner].properties.len() + 1;
        let firm = self.estate.owners[owner].name.clone();
        let name = format!("{firm} #{n}");
        let branch = if taxi { self.build_taxi_company(lot, name.clone())? } else { self.build_on_as(lot, Some(kind))? };
        if !taxi {
            self.buildings[branch].name = name;
        }
        if let Some(z) = self.buildings.get_mut(bid).and_then(|b| b.business.as_mut()) {
            z.money -= 1500.0;
        }
        self.give_to(branch, owner, 1500.0);
        self.estate.stats.branches_total += 1;
        Some(branch)
    }

    /// The firm at `bid` buys the plot next door (two for oil and farms),
    /// even landlocked backland, and rebuilds across them as one: an oil
    /// field without a fence between wells, a bigger factory or datacentre,
    /// a mega hotel, a gallery grown into a museum. True if it bought.
    pub fn expand_into_neighbours(&mut self, bid: BuildingId) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        let Some(kind) = b.business.as_ref().map(|z| z.kind) else { return false };
        let Some(form) = expansion_form(kind) else { return false };
        if !b.plot.pending_lots.is_empty() || b.construction.is_some() || b.plot.tower.is_some() {
            return false;
        }
        let ours = b.plot.lots(b.lot);
        if ours.len() >= MAX_MERGED_LOTS {
            return false;
        }
        let lu = self.lots.get(b.lot).map_or(b.land_use, |l| l.land_use);
        let floors = b.floors;
        let owner = self.firm_of(bid);
        let want = if matches!(kind, BusinessKind::OilWell | BusinessKind::GrainFarm | BusinessKind::VertFarm | BusinessKind::Dairy) { 2 } else { 1 };
        let mut picked: Vec<LotId> = Vec::new();
        for (_, lid) in self.plots_next_to(&ours, lu, floors.saturating_add(1), owner) {
            if picked.len() >= want || ours.len() + picked.len() >= MAX_MERGED_LOTS {
                break;
            }
            let all: Vec<&Lot> = ours.iter().chain(picked.iter()).chain(std::iter::once(&lid)).skip(1).filter_map(|l| self.lots.get(*l)).collect();
            if merged_lot(&self.lots[ours[0]], &all).is_none() {
                continue;
            }
            picked.push(lid);
        }
        if picked.is_empty() {
            return false;
        }
        let price: f64 = picked.iter().filter_map(|l| self.lots[*l].building).map(|b| self.sale_price(b)).sum();
        let name = self.buildings[bid].name.clone();
        let tier = self.buildings[bid].tier.max(Tier::for_land_value(self.lot_value(self.buildings[bid].lot)));
        let new_floors = match form {
            Form::MegaHotel => floors.max(5) + 3,
            Form::Museum => floors.clamp(2, 4) + 1,
            Form::Tower => floors,
            _ if lu == LandUse::Industrial => floors,
            _ => floors + 1,
        };
        let value = self.lot_value(self.buildings[bid].lot);
        self.buildings[bid].plot.form = Some(form);
        let memory = format!("{name} bought the plot next door to grow. Our building is coming down.");
        self.take_plots(bid, &picked, &memory);
        self.redevelop_with(bid, tier, new_floors, value, format!("{name} is closed while it rebuilds bigger."), true);
        if let Some(o) = owner {
            if let Some(o) = self.estate.owners.get_mut(o) {
                o.spent += price;
            }
        }
        if let Some(z) = self.buildings.get_mut(bid).and_then(|b| b.business.as_mut()) {
            z.money -= (price as f32 * 0.01).min(z.money.max(0.0));
        }
        self.estate.stats.merges_total += 1;
        true
    }

    // ── homes bought outright ──────────────────────────────────────────────

    /// A well-off family buys two plots side by side in a quiet, good
    /// suburb and builds one big house with a pool: at most one a day, and
    /// one for every six well-off households.
    pub(super) fn build_mcmansions(&mut self) {
        let mut rng = self.erng(3);
        if !rng.random_bool(0.3) {
            return;
        }
        let rich: Vec<(f32, HouseholdId)> = {
            let owners: Vec<HouseholdId> = self.estate.owners.values().filter_map(|o| if let OwnerKind::Household { household } = o.kind { Some(household) } else { None }).collect();
            let mut v: Vec<(f32, HouseholdId)> = self
                .households
                .keys()
                .filter(|h| !owners.contains(h))
                .filter(|h| self.households[*h].members.iter().all(|m| !self.estate.is_elite(*m)))
                .map(|h| (self.household_income(h), h))
                .filter(|(i, _)| *i >= 220.0)
                .collect();
            v.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            v
        };
        let built = self.buildings.values().filter(|b| b.plot.form == Some(Form::McMansion)).count();
        if rich.is_empty() || built >= 1 + rich.len() / 6 {
            return;
        }
        let Some(pair) = self.mcmansion_site() else { return };
        let hid = rich[0].1;
        let family = self.households[hid].members.first().and_then(|m| self.citizens.get(*m)).map(|c| c.name.split_whitespace().last().unwrap_or("").to_string()).unwrap_or_default();
        let owner = self.new_owner(format!("the {family} family"), OwnerKind::Household { household: hid });
        if let Some(bid) = self.acquire(&pair, Form::McMansion, owner, Some(hid), 2, &format!("The {family} family bought our place to build a big house. We had to go.")) {
            self.estate.stats.mcmansions_total += 1;
            let _ = bid;
        }
    }

    /// Two touching residential plots in a low-density, well-valued block,
    /// empty or holding low homes the market may buy, that make one compact
    /// plot. The best valued first.
    fn mcmansion_site(&self) -> Option<Vec<LotId>> {
        let mut cands: Vec<(f32, LotId)> = Vec::new();
        for (lid, l) in self.lots.iter() {
            if l.land_use != LandUse::Residential || self.estate.reserved.contains_key(&lid) || self.under_facility(l.center) || self.district_frozen(lid) {
                continue;
            }
            if self.pressure_floors(lid) > LOW_DENSITY_FLOORS {
                continue;
            }
            let value = self.land_value.for_use(l.center, LandUse::Residential);
            if value < 0.45 {
                continue;
            }
            let ok = match l.building {
                None => true,
                Some(b) => self.buyable(b) && self.buildings[b].floors <= 2 && self.buildings[b].tier != Tier::High && self.buildings[b].plot.owner.is_none() && !self.buildings[b].plot.merged(),
            };
            if ok {
                cands.push((value, lid));
            }
        }
        cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        cands.truncate(60);
        for i in 0..cands.len() {
            let a = &self.lots[cands[i].1];
            if a.landlocked || !a.car_access() {
                continue;
            }
            for j in 0..cands.len() {
                if i == j {
                    continue;
                }
                let b = &self.lots[cands[j].1];
                if a.center.distance(b.center) < 90.0 && lots_touch(a, b) && merged_lot(a, &[b]).is_some() {
                    return Some(vec![cands[i].1, cands[j].1]);
                }
            }
        }
        None
    }

    /// Buy `plots` for one owner-occupied house of `form`: the first plot
    /// with a building on it is rebuilt (or, all being empty, a site opens
    /// on the first), the rest are cleared and join it. `buyer` moves in
    /// when it is done.
    fn acquire(&mut self, plots: &[LotId], form: Form, owner: OwnerId, buyer: Option<HouseholdId>, floors: u8, memory: &str) -> Option<BuildingId> {
        let primary = plots.iter().copied().find(|l| self.lots.get(*l).is_some_and(|x| x.building.is_some() && !x.landlocked)).or_else(|| plots.iter().copied().find(|l| self.lots.get(*l).is_some_and(|x| !x.landlocked)))?;
        let price: f64 = plots.iter().filter_map(|l| self.lots.get(*l).and_then(|x| x.building)).map(|b| self.sale_price(b)).sum();
        let value = self.lot_value(primary);
        let bid = match self.lots[primary].building {
            Some(b) => {
                // [relocate] A business on the plot looks for a new place.
                let why = match form {
                    Form::Mansion => "a billionaire's estate".to_string(),
                    _ => "a family's new house".to_string(),
                };
                self.pack_business(b, &why);
                self.buildings[b].plot.form = Some(form);
                self.buildings[b].plot.buyer = buyer;
                self.buildings[b].land_use = LandUse::Residential;
                self.buildings[b].business = None;
                self.redevelop_with(b, Tier::High, floors, value, memory.to_string(), false);
                b
            }
            None => {
                let seed = self.fresh_seed();
                let name = {
                    let mut rng = self.erng(9);
                    names::building_name(&mut rng, LandUse::Residential)
                };
                let mut b = Building::new(primary, LandUse::Residential, floors, Tier::High, seed, name, self.clock.tick, value);
                b.plot.form = Some(form);
                b.plot.buyer = buyer;
                b.construction = Some(Construction::new(Construction::days_for(floors + 2, Tier::High)));
                b.recompute_capacity();
                b.update_rent(value);
                let id = self.buildings.insert(b);
                self.lots[primary].building = Some(id);
                self.events.push(SimEvent::BuildingBuilt(id));
                self.order_materials(id);
                id
            }
        };
        if form == Form::Mansion {
            let n = self.estate.owners.get(owner).map(|o| o.name.clone()).unwrap_or_default();
            self.buildings[bid].name = format!("{n} Estate");
        }
        let rest: Vec<LotId> = plots.iter().copied().filter(|l| *l != primary).collect();
        self.take_plots(bid, &rest, memory);
        self.give_to(bid, owner, price);
        Some(bid)
    }

    /// A building finished: whoever bought it moves in; a mansion starts
    /// its NIMBY pull. Called from `finish_construction`.
    pub(super) fn estate_finished(&mut self, bid: BuildingId) {
        // [supertall] A league tower tops out: the record book and the news.
        self.tower_finished(bid);
        let Some(b) = self.buildings.get(bid) else { return };
        let (form, buyer) = (b.plot.form, b.plot.buyer);
        if let Some(hid) = buyer.filter(|h| self.households.contains_key(*h)) {
            if self.households[hid].home != Some(bid) {
                let free = self.buildings[bid].free_homes() > 0;
                if free {
                    self.move_household(hid, bid, "Moved into our own place: {}. Ours, bought outright.");
                }
            }
        }
        if form == Some(Form::Mansion) {
            let center = self.footprint(bid).map(|l| l.center).unwrap_or_default();
            self.estate.nimby.retain(|n| n.mansion != bid);
            self.estate.nimby.push(Nimby { center, radius: NIMBY_RADIUS.min(NIMBY_MAX_RADIUS), strength: NIMBY_LIFT, mansion: bid, price_out_left: NIMBY_PRICE_OUT_CAP });
            self.estate.stats.mansions_total += 1;
        }
        // [rich] The garage fills.
        self.fill_garage(bid);
    }

    // ── the super-elite ────────────────────────────────────────────────────

    /// The disparity a billionaire needs: (population, Gini, top twentieth
    /// over the median), and whether it is extreme enough.
    pub fn elite_gate(&self) -> (usize, f32, f32, bool) {
        let pop = self.citizens.len();
        let mut incomes: Vec<f32> = self.citizens.values().filter(|c| c.stage() != crate::citizens::LifeStage::Child).map(|c| c.income).collect();
        incomes.sort_by(|a, b| a.total_cmp(b));
        let ratio = if incomes.len() >= 20 {
            let median = incomes[incomes.len() / 2].max(1.0);
            let top = &incomes[incomes.len() - incomes.len() / 20..];
            top.iter().sum::<f32>() / top.len() as f32 / median
        } else {
            0.0
        };
        let gini = self.economy.gini;
        (pop, gini, ratio, pop >= ELITE_MIN_POP && gini >= ELITE_MIN_GINI && ratio >= ELITE_MIN_TOP_RATIO)
    }

    pub fn elite_cap(&self) -> usize {
        (self.citizens.len() / ELITE_PEOPLE_EACH).clamp(1, ELITE_MAX)
    }

    pub(super) fn daily_elites(&mut self) {
        // Their fortune pays them whatever job they took; the supercar is
        // brought home for the morning.
        for i in 0..self.estate.elites.len() {
            let e = &self.estate.elites[i];
            let (cid, income) = (e.citizen, elite_income(e.net_worth));
            if let Some(c) = self.citizens.get_mut(cid) {
                c.income = c.income.max(income);
            }
            if let Some(at) = self.citizens.get(cid).and_then(|c| c.home).and_then(|h| self.building_entrance(h)) {
                self.bring_car(cid, at);
            }
        }
        if self.estate.elites.len() >= self.elite_cap() || !self.elite_gate().3 {
            return;
        }
        let mut rng = self.erng(4);
        let chance = (ELITE_DAILY_CHANCE * (1.0 + 0.5 * self.estate.elites.len() as f64)).min(0.25);
        if rng.random_bool(chance) {
            let _ = self.spawn_elite(&mut rng);
        }
    }

    /// A billionaire buys a quiet cluster of residential plots and builds a
    /// mansion; a local made rich, or a visitor who stays. Their other
    /// homes, businesses and cars come with them. Errors when there is no
    /// quiet land or nobody to be one. (Called by the daily roll once the
    /// disparity holds, and by the remote API's dev command.)
    pub fn spawn_elite(&mut self, rng: &mut SimRng) -> Result<CitizenId, String> {
        if self.estate.elites.len() >= ELITE_MAX {
            return Err("the town has all the billionaires it can carry".into());
        }
        let site = self.elite_site_why()?;
        let worth = 10f64.powf(9.0 + rng.random::<f64>() * 1.65);
        // A local who made it, else a visitor who stayed.
        let local = self
            .citizens
            .iter()
            .filter(|(id, c)| c.stage() == crate::citizens::LifeStage::Adult && c.class() == Tier::High && c.household.is_some() && !self.estate.is_elite(*id))
            .max_by(|a, b| a.1.income.total_cmp(&b.1.income).then(b.0.cmp(&a.0)))
            .map(|(id, _)| id);
        let cid = match local.filter(|_| rng.random_bool(0.5)) {
            Some(c) => c,
            None => self.elite_visitor(rng).or(local).ok_or("nobody to be one")?,
        };
        let origin = if local == Some(cid) { EliteOrigin::Local } else { EliteOrigin::Visitor };
        let name = self.citizens[cid].name.clone();
        let owner = self.new_owner(name.clone(), OwnerKind::Elite { citizen: cid });
        let income = elite_income(worth);
        let tick = self.clock.tick;
        {
            let c = &mut self.citizens[cid];
            c.income = income;
            c.savings += 5_000.0;
            c.remember(tick, format!("{} Time for a place worthy of it.", if origin == EliteOrigin::Local { "Made my first billion here." } else { "Came for a visit. Bought half a street." }));
        }
        if let Some(h) = self.citizens[cid].household.and_then(|h| self.households.get_mut(h)) {
            h.money += 50_000.0;
        }
        // The garage: two to six supercars, the first driven every day.
        let ncars = rng.random_range(2..=6usize);
        let cars: Vec<VehicleModel> = (0..ncars).map(|k| supercar(self.citizens[cid].seed ^ k as u64)).collect();
        self.give_supercar(cid, cars[0]);
        let hid = self.citizens[cid].household;
        let mansion = self.acquire(&site, Form::Mansion, owner, hid, 2, &format!("{name} bought the whole row for a mansion. Paid us off and had us out in a day."));
        // Other homes and businesses elsewhere.
        let homes: Vec<BuildingId> = {
            let mut v: Vec<(f32, BuildingId)> = self
                .buildings
                .iter()
                .filter(|(id, b)| b.land_use == LandUse::Residential && b.tier == Tier::High && b.plot.owner.is_none() && Some(*id) != mansion && self.buyable(*id))
                .map(|(id, b)| (b.land_value, id))
                .collect();
            v.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            v.into_iter().take(rng.random_range(1..=2usize)).map(|(_, id)| id).collect()
        };
        let firms: Vec<BuildingId> = {
            let mut v: Vec<(f32, BuildingId)> = self
                .buildings
                .iter()
                .filter(|(id, b)| b.business.as_ref().is_some_and(|z| !z.closed) && b.plot.owner.is_none() && self.buyable(*id))
                .map(|(id, b)| (b.business.as_ref().map_or(0.0, |z| z.money), id))
                .collect();
            v.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            v.into_iter().take(rng.random_range(1..=3usize)).map(|(_, id)| id).collect()
        };
        for b in homes.into_iter().chain(firms) {
            let p = self.sale_price(b);
            self.give_to(b, owner, p);
        }
        let escorts = Elite::escorts_for(worth);
        let day = self.clock.day();
        self.estate.elites.push(Elite { citizen: cid, owner, net_worth: worth, origin, mansion, cars, escorts, police: false, since_day: day as u32 });
        self.refresh_police_escort();
        self.estate.stats.elites_total += 1;
        Ok(cid)
    }

    /// Only the richest, and only from `POLICE_ESCORT_WORTH`, ride behind
    /// the police.
    fn refresh_police_escort(&mut self) {
        let top = self.estate.elites.iter().enumerate().max_by(|a, b| a.1.net_worth.total_cmp(&b.1.net_worth)).map(|(i, e)| (i, e.net_worth));
        for (i, e) in self.estate.elites.iter_mut().enumerate() {
            e.police = top.is_some_and(|(t, w)| t == i && w >= POLICE_ESCORT_WORTH);
        }
    }

    /// A visitor who stays: a new one-person household, put up in the best
    /// free home until the mansion is done. `None` without a free home.
    fn elite_visitor(&mut self, rng: &mut SimRng) -> Option<CitizenId> {
        let home = self.buildings.iter().filter(|(_, b)| b.land_use == LandUse::Residential && b.operating() && b.free_homes() > 0).max_by(|a, b| a.1.tier.cmp(&b.1.tier).then(a.1.rent.total_cmp(&b.1.rent)).then(b.0.cmp(&a.0))).map(|(id, _)| id)?;
        let template = self.citizens.values().find(|c| c.stage() == crate::citizens::LifeStage::Adult)?.clone();
        let seed = self.fresh_seed();
        let family = names::family_name(rng);
        let name = names::member_name(rng, &family);
        let pos = self.building_entrance(home).unwrap_or_default();
        let tick = self.clock.tick;
        let hid = self.households.insert(Household { members: Vec::new(), home: Some(home), money: 50_000.0, groceries: 6.0, rent_stress_days: 0, moved_day: self.clock.day() });
        let look = crate::looks::family_looks(&[(seed, 48)], Tier::High).into_iter().next();
        let c = Citizen {
            name,
            seed,
            home: Some(home),
            work: None,
            pos,
            prev_pos: pos,
            heading: 0.0,
            prev_heading: 0.0,
            mode: TravelMode::Walk,
            owns_car: false,
            owns_bike: false,
            vehicle: None,
            parked_at: None,
            transit: None,
            activity: Activity::Idle,
            memories: Vec::new(),
            places: Default::default(),
            thought: "Charming little city. I'll take it.".to_string(),
            spotlight: false,
            next_decision_tick: tick + 5,
            arrived_tick: tick,
            inside: None,
            mood: 0.8,
            income: 1000.0,
            savings: 5_000.0,
            rent_stress_days: 0,
            satiety: 0.9,
            groceries: 6.0,
            hungry_days: 0,
            trail: Vec::new(),
            pending_footfall: Vec::new(),
            household: Some(hid),
            age: 48,
            car: None,
            to_car: None,
            look,
            ..template
        };
        let id = self.citizens.insert(c);
        self.households[hid].members.push(id);
        self.buildings[home].households.push(hid);
        self.buildings[home].residents.push(id);
        self.events.push(SimEvent::CitizenArrived(id));
        Some(id)
    }

    /// The citizen's everyday car becomes a supercar (or one is bought and
    /// parked at home).
    pub(super) fn give_supercar(&mut self, cid: CitizenId, model: VehicleModel) {
        let Some(c) = self.citizens.get(cid) else { return };
        if let Some(vid) = c.car.filter(|v| self.vehicles.cars.contains_key(*v)) {
            self.vehicles.cars[vid].model = model;
            let c = &mut self.citizens[cid];
            c.vehicle = Some(model);
            c.owns_car = true;
            return;
        }
        let near = c.home.and_then(|h| self.building_entrance(h)).unwrap_or(c.pos);
        let home = c.home;
        let Some(spot) = self.place_near(near, home) else { return };
        let vid = self.new_car(cid, model, crate::vehicle::CarPlace::Driving(CarId::default()), crate::vehicle::CarEvent::Bought);
        self.park_at(vid, spot);
    }

    /// The best quiet cluster of two to three touching residential plots:
    /// low pressure, no nuisance or fouled ground, little built round it,
    /// no industry or nightlife near; land value counts but parks hardly
    /// (the grounds are the park), and other mansions draw them.
    pub fn elite_site(&self) -> Option<Vec<LotId>> {
        self.elite_site_why().ok()
    }

    /// `elite_site`, or why there is none: how many residential plots
    /// passed each test.
    pub fn elite_site_why(&self) -> Result<Vec<LotId>, String> {
        let (mut open_n, mut quiet_n, mut clear_n) = (0usize, 0usize, 0usize);
        let loud: Vec<Vec2> = self
            .buildings
            .values()
            .filter(|b| matches!(b.land_use, LandUse::Industrial | LandUse::Entertainment) || b.plant.is_some())
            .filter_map(|b| self.lots.get(b.lot).map(|l| l.center))
            .collect();
        let mansions: Vec<Vec2> = self.buildings.iter().filter(|(_, b)| b.plot.form == Some(Form::Mansion)).filter_map(|(id, _)| self.footprint(id).map(|l| l.center)).collect();
        let open = |lid: LotId, l: &Lot| -> bool {
            if l.land_use != LandUse::Residential || self.estate.reserved.contains_key(&lid) || self.under_facility(l.center) || self.district_frozen(lid) {
                return false;
            }
            match l.building {
                None => true,
                Some(b) => self.buildings.get(b).is_some_and(|x| x.floors <= 2 && x.tier != Tier::High && x.plot.owner.is_none() && !x.plot.merged()) && self.buyable(b),
            }
        };
        let quiet_max = {
            let mut v: Vec<f32> = self.lots.values().filter(|l| l.building.is_some()).map(|l| self.estate.pressure.at(l.center)).collect();
            v.sort_by(|a, b| a.total_cmp(b));
            let median = v.get(v.len() / 2).copied().unwrap_or(0.0);
            ELITE_QUIET_PRESSURE.max(median * ELITE_QUIET_SHARE)
        };
        let mut cands: Vec<(f32, LotId)> = Vec::new();
        for (lid, l) in self.lots.iter() {
            if !open(lid, l) {
                continue;
            }
            open_n += 1;
            let pressure = self.estate.pressure.at(l.center);
            if pressure > quiet_max || self.land_value.nuisance_at(l.center) > ELITE_MAX_NUISANCE || self.contamination.at(l.center) > ELITE_MAX_CONTAMINATION {
                continue;
            }
            quiet_n += 1;
            if loud.iter().any(|p| p.distance(l.center) < ELITE_NUISANCE_CLEARANCE) || mansions.iter().any(|m| m.distance(l.center) < 90.0) {
                continue;
            }
            clear_n += 1;
            // Parks matter less: most of the walking-access term comes out.
            let value = self.land_value.at(l.center) - 0.8 * self.land_value.access_value_at(l.center);
            let quiet = (quiet_max - pressure) / quiet_max * 0.3;
            let company = if mansions.iter().any(|m| m.distance(l.center) < ELITE_ATTRACT_RADIUS) { 0.4 } else { 0.0 };
            cands.push((value + quiet + company, lid));
        }
        cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        // Partners may be any quiet plot; the best forty seed the search.
        let pool: Vec<LotId> = cands.iter().map(|c| c.1).collect();
        cands.truncate(40);
        for &(_, seed) in &cands {
            let s = &self.lots[seed];
            if s.landlocked || !s.car_access() {
                continue;
            }
            // Little built round it.
            let near_floors: u32 = self.buildings.values().filter(|b| b.land_use != LandUse::Park).filter_map(|b| self.lots.get(b.lot).map(|l| (b.floors as u32, l.center))).filter(|(_, c)| c.distance(s.center) <= ELITE_DENSITY_RADIUS).map(|(f, _)| f).sum();
            if near_floors > ELITE_MAX_NEARBY_FLOORS {
                continue;
            }
            let mut cluster = vec![seed];
            let mut area = s.area();
            for &other in &pool {
                if cluster.len() >= MAX_MERGED_LOTS || area >= ELITE_WANT_AREA {
                    break;
                }
                if cluster.contains(&other) {
                    continue;
                }
                let o = &self.lots[other];
                if !cluster.iter().any(|c| self.lots[*c].center.distance(o.center) < 100.0 && lots_touch(&self.lots[*c], o)) {
                    continue;
                }
                let others: Vec<&Lot> = cluster.iter().skip(1).chain(std::iter::once(&other)).map(|l| &self.lots[*l]).collect();
                if merged_lot(s, &others).is_some() {
                    cluster.push(other);
                    area += o.area();
                }
            }
            if cluster.len() >= 2 && area >= ELITE_MIN_AREA {
                return Ok(cluster);
            }
        }
        Err(format!("no quiet land for a mansion: {open_n} open residential plots, {quiet_n} quiet (pressure <= {quiet_max:.1}, no nuisance or fouled ground), {clear_n} clear of works and nightlife; none with a touching quiet neighbour and little built round it"))
    }

    /// NIMBY: the lift fades; while it holds, up to two cheap homes a day
    /// in the radius are bought and rebuilt dearer (never taller than
    /// `NIMBY_HEIGHT_CAP`), at most `NIMBY_PRICE_OUT_CAP` per mansion.
    pub(super) fn daily_nimby(&mut self) {
        let buildings = &self.buildings;
        self.estate.nimby.retain(|n| buildings.contains_key(n.mansion));
        for n in &mut self.estate.nimby {
            n.strength *= NIMBY_DECAY;
        }
        self.estate.nimby.retain(|n| n.strength >= 0.02);
        let mut rng = self.erng(5);
        for k in 0..self.estate.nimby.len() {
            let n = self.estate.nimby[k].clone();
            if n.price_out_left == 0 {
                continue;
            }
            let who = self.estate.elites.iter().find(|e| e.mansion == Some(n.mansion)).and_then(|e| self.citizens.get(e.citizen)).map(|c| c.name.clone()).unwrap_or_else(|| "a billionaire".into());
            let mut cheap: Vec<(f32, BuildingId)> = self
                .buildings
                .iter()
                .filter(|(id, b)| b.land_use == LandUse::Residential && b.tier == Tier::Low && b.plot.form.is_none() && self.buyable(*id))
                .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (l.center.distance(n.center), id)))
                .filter(|(d, _)| *d < n.radius)
                .collect();
            cheap.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let mut done = 0u8;
            for (_, bid) in cheap {
                if done >= NIMBY_PRICE_OUT_PER_DAY || self.estate.nimby[k].price_out_left == 0 {
                    break;
                }
                if !rng.random_bool(0.6) {
                    continue;
                }
                let lot = self.buildings[bid].lot;
                let value = self.lot_value(lot);
                let floors = self.buildings[bid].floors.clamp(1, NIMBY_HEIGHT_CAP);
                let memory = format!("Priced out: {who} built a mansion down the road and the landlord sold up. Rents here are for someone else now.");
                self.redevelop_with(bid, Tier::Mid, floors, value, memory, false);
                self.estate.nimby[k].price_out_left -= 1;
                self.estate.stats.priced_out_total += 1;
                done += 1;
            }
        }
    }

    // ── convoys ────────────────────────────────────────────────────────────

    /// Every tick: a billionaire who sets off in their car gets the convoy
    /// (black cars behind, a police car ahead for the richest); a convoy
    /// whose boss has parked leaves guards at the kerb for an hour and the
    /// escorts drive on to the end of the route and leave.
    pub(super) fn step_convoys(&mut self) {
        if self.estate.elites.is_empty() && self.estate.convoys.is_empty() {
            return;
        }
        for i in 0..self.estate.elites.len() {
            let cid = self.estate.elites[i].citizen;
            let Some(Activity::Driving { car, .. }) = self.citizens.get(cid).map(|c| &c.activity) else { continue };
            let car = *car;
            if self.traffic.cars.contains_key(car) && !self.estate.convoys.iter().any(|c| c.boss == car) {
                self.form_convoy(i, car);
            }
        }
        // The boss changed course (a new errand, a parking hunt): the
        // detail goes where it goes, from where each car stands.
        let mut turns: Vec<(CarId, crate::traffic::LaneId, f32)> = Vec::new();
        for conv in &self.estate.convoys {
            let Some(b) = self.traffic.cars.get(conv.boss) else { continue };
            let (Some(&goal), s_end) = (b.route.last(), b.s_end) else { continue };
            for id in conv.escorts.iter().chain(conv.police.iter()) {
                if self.traffic.cars.get(*id).is_some_and(|c| c.route.last() != Some(&goal)) {
                    turns.push((*id, goal, s_end));
                }
            }
        }
        for (id, goal, s_end) in turns {
            self.traffic.redirect(id, goal, s_end);
        }
        // Escorts drive like a trained detail: the boss's pace, a touch
        // quicker to close up, a short headway, and through an amber when
        // the supercar went.
        for conv in &self.estate.convoys {
            let Some((boss, at)) = self.traffic.cars.get(conv.boss).map(|b| (b.drive.profile, b.pos)) else { continue };
            if !boss.assigned {
                continue;
            }
            for id in conv.escorts.iter().chain(conv.police.iter()) {
                if let Some(car) = self.traffic.cars.get_mut(*id) {
                    let mut p = boss;
                    // A detail keeps together through the lights, and one
                    // left behind closes up fast.
                    let behind = car.pos.distance(at);
                    p.speed = boss.speed * if behind > 60.0 { 1.3 } else { 1.06 };
                    p.ignores_signals = true;
                    p.headway = (boss.headway * 0.7).max(0.8);
                    p.accel = boss.accel * 1.1;
                    p.reaction = 0.4;
                    p.compliance = boss.compliance.min(0.5);
                    p.skill = 1.0;
                    p.drunk = 0.0;
                    car.drive.profile = p;
                }
            }
        }
        let tick = self.clock.tick;
        let cars = &self.traffic.cars;
        let citizens = &self.citizens;
        for c in &mut self.estate.convoys {
            c.escorts.retain(|e| cars.contains_key(*e));
            c.police = c.police.filter(|p| cars.contains_key(*p));
            if c.stop.is_none() && !cars.contains_key(c.boss) {
                let at = citizens.get(c.citizen).map_or(Vec2::ZERO, |x| x.pos);
                c.stop = Some((at, tick + 60));
            }
        }
        self.estate.convoys.retain(|c| cars.contains_key(c.boss) || !c.escorts.is_empty() || c.police.is_some() || c.stop.is_some_and(|(_, t)| t > tick));
    }

    fn form_convoy(&mut self, elite: usize, boss: CarId) {
        let Some(car) = self.traffic.cars.get(boss) else { return };
        if car.idx >= car.route.len() {
            return;
        }
        let route: Vec<crate::traffic::LaneId> = car.route[car.idx..].to_vec();
        let (s, s_end) = (car.s, car.s_end);
        let e = &self.estate.elites[elite];
        let (cid, n, police) = (e.citizen, e.escorts, e.police);
        // Tight behind the supercar, one car length and a gap apart, so
        // they pull out on its tail rather than waiting for a gap of their
        // own; the police car a little ahead.
        let lane_len = self.traffic.net.lanes.get(route[0] as usize).map_or(s, |l| l.length);
        let mut escorts = Vec::new();
        for k in 0..n {
            let at = (s - 7.5 * (k as f32 + 1.0)).max(0.0);
            let id = self.traffic.spawn_car(CitizenId::default(), None, route.clone(), at, s_end, 0);
            if let Some(c) = self.traffic.cars.get_mut(id) {
                c.model = escort_model();
            }
            escorts.push(id);
        }
        let police = police.then(|| {
            let ahead = if route.len() > 1 { (s + 9.0).min(lane_len - 0.5).max(s) } else { s };
            let id = self.traffic.spawn_car(CitizenId::default(), None, route.clone(), ahead, s_end, 3);
            if let Some(c) = self.traffic.cars.get_mut(id) {
                c.model = police_escort_model();
            }
            id
        });
        self.estate.convoys.push(Convoy { citizen: cid, boss, escorts, police, stop: None });
        self.estate.stats.convoys_total += 1;
    }

    /// An escort or police car of a convoy reached the end of its route:
    /// it leaves the road (it drives off to wherever such cars wait).
    pub(super) fn convoy_car_arrived(&mut self, car: CarId) -> bool {
        let Some(conv) = self.estate.convoys.iter().find(|c| c.escorts.contains(&car) || c.police == Some(car)) else { return false };
        // The boss drives on (a parking hunt, a new errand): so does the
        // detail.
        if let Some(goal) = self.traffic.cars.get(conv.boss).and_then(|b| b.route.last().map(|g| (*g, b.s_end))) {
            if self.traffic.redirect(car, goal.0, goal.1) && self.traffic.cars.get(car).is_some_and(|c| c.idx + 1 < c.route.len() || c.s + 1.0 < c.s_end) {
                return true;
            }
        }
        for c in &mut self.estate.convoys {
            c.escorts.retain(|e| *e != car);
            if c.police == Some(car) {
                c.police = None;
            }
        }
        self.traffic.remove_car(car);
        true
    }

    /// The driver brings a billionaire's supercar round to `near` when it
    /// stands parked far off (they never walk to the car).
    pub(super) fn bring_car(&mut self, cid: CitizenId, near: Vec2) {
        let Some(vid) = self.citizens.get(cid).and_then(|c| c.car) else { return };
        let far = self.vehicles.cars.get(vid).and_then(|c| c.spot()).is_some_and(|s| s.pos.distance(near) > 120.0);
        if !far {
            return;
        }
        let home = self.citizens[cid].home;
        let Some(spot) = self.place_near(near, home) else { return };
        self.unpark(vid);
        self.park_at(vid, spot);
    }

    /// Dev and test hook: send a citizen somewhere (a billionaire's car is
    /// brought round first, so they drive when it is far enough).
    pub fn send_citizen(&mut self, cid: CitizenId, to: BuildingId) -> bool {
        if !self.citizens.contains_key(cid) || !self.buildings.contains_key(to) {
            return false;
        }
        if self.estate.is_elite(cid) {
            let at = self.citizens[cid].pos;
            self.bring_car(cid, at);
        }
        self.travel_to(cid, Some(to), Purpose::Visit(LandUse::Commercial));
        true
    }
}

/// Daily income of a fortune: enough to stand out of any income table
/// without swamping the town's books.
fn elite_income(net_worth: f64) -> f32 {
    (2_000.0 + (net_worth / 1e9) as f32 * 120.0).min(8_000.0)
}

/// A billionaire's supercar: a souped-up, glossy coupe.
pub fn supercar(seed: u64) -> VehicleModel {
    use crate::vehicle::{BodyType, Livery, VehicleClass, PAINTS};
    VehicleModel { class: VehicleClass::Expensive, body: BodyType::Coupe, wear: 0, tuned: true, livery: Livery::None, paint: (seed % PAINTS as u64) as u8 }
}

/// The black SUVs of an escort.
pub fn escort_model() -> VehicleModel {
    use crate::vehicle::{BodyType, Livery, VehicleClass};
    VehicleModel { class: VehicleClass::Expensive, body: BodyType::Suv, wear: 0, tuned: false, livery: Livery::None, paint: 0 }
}

/// The police car leading the richest one's convoy (the renderer adds the
/// light bar: `render/convoys.rs`).
pub fn police_escort_model() -> VehicleModel {
    use crate::vehicle::{BodyType, Livery, VehicleClass};
    VehicleModel { class: VehicleClass::Mid, body: BodyType::Sedan, wear: 0, tuned: false, livery: Livery::None, paint: 3 }
}
