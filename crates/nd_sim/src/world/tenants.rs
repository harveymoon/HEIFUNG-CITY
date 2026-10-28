//! [tenants] Several businesses in one building. A building that can be let
//! in parts is cut into units when it opens (`World::plan_units`): shopfronts
//! along its street front on the ground floor (under flats, under offices, a
//! two-storey mall base under a supertall) and, in an office or shop
//! building, ranges of upper floors. Its own business (`Building::business`)
//! has the units its kind and staff want; the rest are let, each to a
//! business of its own: a tenant, a record in `World::buildings` with
//! `unit_of` set to the host, so it hires, sells, orders, keeps books and an
//! account like any business, but stands on the host's lot, is drawn as the
//! host and goes when the host goes.
//!
//! Nobody assigns the space: owners let empty units to whatever the market
//! would open (`World::let_units`, the same reading as a new building's
//! business), a business doing well takes the empty unit next door
//! (`World::grow_into_units`), one that shuts frees its units, and a
//! business that loses its building moves into an empty unit nearby before
//! it builds anew (`relocate.rs`). Tenants pay their owner rent by floor
//! area (`World::shop_rent`).
//!
//! No draws from the world's stream: every pick is a hash of the building's
//! seed and the day.

use super::*;
use crate::buildings::{Occupant, Unit, UnitRole};
use crate::estate::OwnerKind;
use crate::hash::hash01;

/// Street front a shopfront takes, m.
pub const SHOPFRONT_WIDTH: f32 = 14.0;
/// Shopfronts along one street front at most.
pub const MAX_SHOPFRONTS: usize = 4;
/// Shops in a supertall's mall base: four, two more for each extra plot of
/// its podium, at most this many.
pub const MAX_MALL_SHOPS: usize = 10;
/// Share of a plot a building covers, for its floor area.
pub const BUILT_COVER: f32 = 0.55;
/// A shop as its building's own business keeps upper floors only up to
/// this many jobs; the floors above are let as offices.
pub const RETAIL_MAX_JOBS: u16 = 24;
/// Floor area of a home, m²: a unit's rent per m² is a home's rent here
/// over this, times [`UNIT_RENT_SHARE`].
pub const HOME_M2: f32 = 70.0;
/// A business unit's rent per m² against a home's in the same building.
pub const UNIT_RENT_SHARE: f32 = 0.12;
/// Chance a day that the owner lets an empty unit, at full demand.
pub const LET_CHANCE: f32 = 0.3;
/// Units let across the city in a day, at most.
pub const LETS_PER_DAY: usize = 8;
/// Units taken by growing businesses in a day, at most.
pub const GROWS_PER_DAY: usize = 4;
/// Days a shut tenant's sign stays up before its unit is to let again.
pub const SHUT_DAYS: u8 = 3;

/// Kinds that fit a shopfront: shops customers walk into.
pub fn fits_shopfront(kind: BusinessKind) -> bool {
    matches!(kind, BusinessKind::Grocery | BusinessKind::Bakery | BusinessKind::GadgetShop | BusinessKind::Cafe | BusinessKind::Bookshop | BusinessKind::BikeShop)
}

/// The units a building would be let in, all empty, from its floors, its
/// street front (`frontage`, m; 0 for backland) and the floor area of one
/// storey (`plate`, m²). Empty for what is one business or none: houses and
/// walk-ups, sheds, yards and forecourts, the city's structures, sites.
pub fn unit_layout(b: &Building, frontage: f32, plate: f32) -> Vec<Unit> {
    if !b.operating() || b.is_special() || b.is_parking() || b.plot.form.is_some_and(|f| f.owner_occupied()) {
        return Vec::new();
    }
    // [malls] A strip mall, mall or mega-mall has its own layout, whatever
    // its own store is.
    if let Some(form) = b.plot.form.filter(|f| f.is_mall()) {
        return super::malls::mall_layout(form, b.floors.max(1), frontage, plate);
    }
    if b.business.as_ref().is_some_and(|z| z.kind.max_floors().is_some() || z.kind.needs_site() || z.kind.industry().is_some()) {
        return Vec::new();
    }
    let tower = b.plot.tower.is_some();
    let floors = b.floors.max(1);
    let ok = match b.land_use {
        LandUse::Residential => tower || (floors >= 3 && frontage > 0.0),
        LandUse::Commercial => tower || floors >= 2,
        _ => false,
    };
    if !ok {
        return Vec::new();
    }
    // A supertall's mall base is its two lowest storeys.
    let (shops, base_top) = if tower {
        ((4 + 2 * b.plot.extra_lots.len()).min(MAX_MALL_SHOPS), 1u8.min(floors - 1))
    } else {
        (((frontage / SHOPFRONT_WIDTH).floor() as usize).clamp(1, MAX_SHOPFRONTS), 0)
    };
    let mut v = Vec::new();
    let share = 1.0 / shops as f32;
    for _ in 0..shops {
        v.push(Unit { lo: 0, hi: base_top, share, area: plate * share * (base_top + 1) as f32, shopfront: true, jobs: 0, occupant: Occupant::Vacant, vacant_since: 0, role: UnitRole::Shop });
    }
    if b.land_use == LandUse::Commercial {
        // Upper floors in ranges: a floor each in a low block, up to eight
        // ranges of three or more in a tower.
        let first = base_top + 1;
        let upper = floors.saturating_sub(first);
        if upper > 0 {
            let band = if upper <= 3 { 1 } else { upper.div_ceil(8).max(3) };
            let mut lo = first;
            while lo < floors {
                let hi = (lo + band - 1).min(floors - 1);
                v.push(Unit { lo, hi, share: 1.0, area: plate * (hi - lo + 1) as f32, shopfront: false, jobs: 0, occupant: Occupant::Vacant, vacant_since: 0, role: UnitRole::Shop });
                lo = hi + 1;
            }
        }
    }
    v
}

/// Give the building's own business its units: a shop the shopfronts and
/// the floors over them up to [`RETAIL_MAX_JOBS`], anything else the upper
/// floors (and the shopfronts too when `keep_shops`: an owner that keeps
/// its street front). With `staff`, at least units enough for that many
/// staff, so no one is laid off by the cut (a save from before units).
/// The unit jobs must be set (`recompute_capacity`).
pub fn assign_main(b: &mut Building, keep_shops: bool, staff: u16) {
    let Some(kind) = b.business.as_ref().map(|z| z.kind) else { return };
    // [malls] A mall's own store has one unit (an anchor if there is one);
    // the rest are let.
    if b.plot.form.is_some_and(|f| f.is_mall()) {
        super::malls::assign_mall_main(b, kind, staff);
        return;
    }
    let shop = fits_shopfront(kind);
    let mut own = 0u16;
    for u in b.units.iter_mut() {
        let take = if u.shopfront { shop || keep_shops } else { !shop || own < RETAIL_MAX_JOBS };
        if take {
            u.occupant = Occupant::Main;
            own += u.jobs;
        }
    }
    // Shops first for a shop, floors first for the rest.
    let mut order: Vec<usize> = (0..b.units.len()).collect();
    order.sort_by_key(|&i| (b.units[i].shopfront != shop, i));
    for i in order {
        if own >= staff {
            break;
        }
        let u = &mut b.units[i];
        if u.vacant() {
            u.occupant = Occupant::Main;
            own += u.jobs;
        }
    }
}

impl World {
    /// [tenants] The building `bid` stands in: its host for a tenant, else
    /// itself.
    pub fn host_of(&self, bid: BuildingId) -> BuildingId {
        self.buildings.get(bid).and_then(|b| b.unit_of).unwrap_or(bid)
    }

    /// [tenants] The businesses renting units in `bid`, in unit order.
    pub fn tenants_of(&self, bid: BuildingId) -> Vec<BuildingId> {
        let mut v: Vec<BuildingId> = Vec::new();
        if let Some(b) = self.buildings.get(bid) {
            for u in &b.units {
                if let Occupant::Tenant(t) = u.occupant {
                    if !v.contains(&t) && self.buildings.contains_key(t) {
                        v.push(t);
                    }
                }
            }
        }
        v
    }

    /// [tenants] Floor area `bid`'s business has: its units, or for a
    /// building that is one business, all of it.
    pub fn business_area(&self, bid: BuildingId) -> f32 {
        let Some(b) = self.buildings.get(bid) else { return 0.0 };
        match b.unit_of {
            Some(h) => self.buildings.get(h).map_or(0.0, |h| h.units.iter().filter(|u| u.occupant == Occupant::Tenant(bid)).map(|u| u.area).sum()),
            None if !b.units.is_empty() => b.units.iter().filter(|u| u.occupant == Occupant::Main).map(|u| u.area).sum(),
            None => self.lots.get(b.lot).map_or(0.0, |l| l.area() * (1 + b.plot.extra_lots.len()) as f32) * BUILT_COVER * b.floors.max(1) as f32,
        }
    }

    /// [tenants] Cut `bid` into units if it can be let in parts and is not
    /// yet (`unit_layout`). `migrate`: a building from a save before units,
    /// whose business keeps units for all its staff.
    pub(super) fn plan_units(&mut self, bid: BuildingId, migrate: bool) {
        let Some(b) = self.buildings.get(bid) else { return };
        if !b.units.is_empty() || b.is_tenant() {
            return;
        }
        let Some(lot) = self.footprint(bid) else { return };
        let frontage = if lot.landlocked { 0.0 } else { lot.front_edge[0].distance(lot.front_edge[1]) };
        let plate = lot.area() * BUILT_COVER;
        let mut units = unit_layout(b, frontage, plate);
        if units.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        for u in units.iter_mut() {
            u.vacant_since = tick;
        }
        // A mega-corp or firm running its own building keeps its street
        // front, or lets it: its own call, by the building's seed.
        let keep_shops = match b.plot.owner.and_then(|o| self.estate.owners.get(o)).map(|o| &o.kind) {
            Some(OwnerKind::MegaCorp { .. } | OwnerKind::Firm { .. }) => hash01(b.seed, 0x5409, 0) < 0.5,
            _ => false,
        };
        let staff = if migrate { b.workers.len() as u16 } else { 0 };
        let b = &mut self.buildings[bid];
        b.units = units;
        b.recompute_capacity();
        assign_main(b, keep_shops, staff);
        b.recompute_capacity();
        // Staff beyond the business's units (none on a migration).
        let over = b.workers.len().saturating_sub(b.jobs as usize);
        let gone: Vec<CitizenId> = (0..over).filter_map(|_| self.buildings[bid].workers.pop()).collect();
        let name = self.buildings[bid].name.clone();
        for c in gone {
            if self.citizens.get(c).is_some_and(|x| x.work == Some(bid)) {
                self.lay_off(c, Some(format!("{name} let part of its floors. Out of a job.")));
            }
        }
        self.events.push(SimEvent::BuildingChanged(bid));
    }

    /// [tenants] A business opens in unit `i` of `host`: a tenant record on
    /// the host's lot, its till opened at the business's float. Returns it.
    pub(super) fn open_tenant(&mut self, host: BuildingId, i: usize, z: Business, name: Option<String>) -> Option<BuildingId> {
        let h = self.buildings.get(host)?;
        let u = *h.units.get(i)?;
        if !u.vacant() || !h.operating() {
            return None;
        }
        let kind = z.kind;
        let seed = crate::hash::mix2(h.seed, 0x7E4A_0000 ^ (i as u64) ^ (self.clock.day() << 8));
        let name = name.or_else(|| names::business_name(kind, seed)).unwrap_or_else(|| format!("{} unit {}", h.name, i + 1));
        let mut t = Building::new(h.lot, LandUse::Commercial, u.floors(), h.tier, seed, name, self.clock.tick, h.land_value);
        t.unit_of = Some(host);
        t.condition = h.condition;
        t.look = h.look;
        t.homes = 0;
        t.jobs = u.jobs;
        t.parking_capacity = 0;
        t.rent = h.rent;
        let tid = self.buildings.insert(t);
        self.buildings[host].units[i].occupant = Occupant::Tenant(tid);
        self.set_business(tid, Some(z));
        self.events.push(SimEvent::BuildingChanged(host));
        Some(tid)
    }

    /// [tenants] Tenant `tid` leaves: its staff are let go (with `memory`),
    /// anyone inside steps out, its units are to let, its record and
    /// account close (what is left in the till leaves town).
    pub(super) fn vacate_tenant(&mut self, tid: BuildingId, memory: Option<String>) {
        let Some(t) = self.buildings.get(tid) else { return };
        let Some(host) = t.unit_of else { return };
        let tick = self.clock.tick;
        let workers = t.workers.clone();
        let out = self.lots.get(t.lot).map(|l| l.entrance());
        for c in workers {
            if self.citizens.get(c).is_some_and(|x| x.work == Some(tid)) {
                self.lay_off(c, memory.clone());
            }
        }
        for c in self.citizens.values_mut().filter(|c| c.inside == Some(tid)) {
            c.inside = None;
            c.activity = Activity::Idle;
            c.next_decision_tick = tick + 1;
            if let Some(p) = out {
                c.pos = p;
                c.prev_pos = p;
            }
        }
        if let Some(h) = self.buildings.get_mut(host) {
            for u in h.units.iter_mut().filter(|u| u.occupant == Occupant::Tenant(tid)) {
                u.occupant = Occupant::Vacant;
                u.vacant_since = tick;
            }
        }
        let lot = self.buildings[tid].lot;
        self.set_business(tid, None);
        self.buildings.remove(tid);
        self.events.push(SimEvent::BuildingRemoved(tid, lot));
        self.events.push(SimEvent::BuildingChanged(host));
    }

    /// [tenants] `host` is coming down or being rebuilt: each tenant packs
    /// up to move (`pack_business_at`, an empty unit or a new building
    /// nearby) or shuts; its staff go now and the units with the building.
    /// The records themselves go at the next `sync_tenants`, so a loop over
    /// the buildings that pulls one down never meets a missing key.
    pub(super) fn evict_tenants(&mut self, host: BuildingId, why: &str) {
        let tenants = self.tenants_of(host);
        let Some(h) = self.buildings.get(host) else { return };
        let name = h.name.clone();
        let from = self.lots.get(h.lot).map_or(Vec2::ZERO, |l| l.center);
        for t in tenants {
            let area = self.business_area(t) / self.buildings[t].floors.max(1) as f32 / BUILT_COVER;
            self.pack_business_at(t, why, from, area, LandUse::Commercial);
            self.retire_tenant(t, Some(format!("{name} is coming down; the shop downstairs had to go.")));
        }
        if let Some(h) = self.buildings.get_mut(host) {
            h.units.clear();
        }
    }

    /// [tenants] Tenant `tid` stops trading: its staff let go (with
    /// `memory`), anyone inside out, its business shut and its units to
    /// let. The record goes at the next `sync_tenants` (`vacate_tenant`).
    pub(super) fn retire_tenant(&mut self, tid: BuildingId, memory: Option<String>) {
        let Some(t) = self.buildings.get_mut(tid) else { return };
        let Some(host) = t.unit_of else { return };
        let workers = std::mem::take(&mut t.workers);
        t.visitors = 0;
        if let Some(z) = t.business.as_mut() {
            z.closed = true;
        }
        for c in workers {
            if self.citizens.get(c).is_some_and(|x| x.work == Some(tid)) {
                self.lay_off(c, memory.clone());
            }
        }
        let tick = self.clock.tick;
        for c in self.citizens.values_mut().filter(|c| c.inside == Some(tid)) {
            c.inside = None;
            c.activity = Activity::Idle;
            c.next_decision_tick = tick + 1;
        }
        if let Some(h) = self.buildings.get_mut(host) {
            for u in h.units.iter_mut().filter(|u| u.occupant == Occupant::Tenant(tid)) {
                u.occupant = Occupant::Vacant;
                u.vacant_since = tick;
            }
        }
        self.events.push(SimEvent::BuildingChanged(host));
    }

    /// [tenants] Every hour: tenants whose host is gone, or no longer
    /// open (a site, coming down), leave; the rest stand as their host
    /// does (its lot, condition, tier, look, rent) and staff the units they
    /// have.
    pub(super) fn sync_tenants(&mut self) {
        let tenants: Vec<(BuildingId, BuildingId)> = self.buildings.iter().filter_map(|(id, b)| b.unit_of.map(|h| (id, h))).collect();
        for (tid, host) in tenants {
            let ok = self.buildings.get(host).is_some_and(|h| h.operating() && h.units.iter().any(|u| u.occupant == Occupant::Tenant(tid)));
            if !ok {
                // Its unit was never freed by `evict_tenants` (a path that
                // removed the host directly): the record goes all the same.
                if let Some(h) = self.buildings.get_mut(host) {
                    h.units.retain(|u| u.occupant != Occupant::Tenant(tid));
                }
                self.vacate_tenant(tid, Some("The building was cleared. Out of a job.".to_string()));
                continue;
            }
            let h = &self.buildings[host];
            let (lot, cond, tier, look, rent, lv) = (h.lot, h.condition, h.tier, h.look, h.rent, h.land_value);
            let jobs: u16 = h.units.iter().filter(|u| u.occupant == Occupant::Tenant(tid)).map(|u| u.jobs).sum();
            let floors = h.units.iter().filter(|u| u.occupant == Occupant::Tenant(tid)).map(|u| u.floors()).sum::<u8>().max(1);
            let t = &mut self.buildings[tid];
            t.lot = lot;
            t.condition = cond;
            t.tier = tier;
            t.look = look;
            t.rent = rent;
            t.land_value = lv;
            t.jobs = jobs;
            t.floors = floors;
        }
    }

    /// [tenants] Once a day, after the businesses' books: shut tenants
    /// leave their units, owners let empty units, growing businesses take
    /// the unit next door.
    pub(super) fn daily_tenants(&mut self) {
        let shut: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.is_tenant() && b.business.as_ref().is_some_and(|z| z.closed && z.broke_days >= SHUT_DAYS)).map(|(id, _)| id).collect();
        for t in shut {
            self.vacate_tenant(t, None);
        }
        self.sync_tenants();
        self.let_units();
        self.grow_into_units();
    }

    /// [tenants] What an empty shopfront in `host` would open as: one of
    /// the shop kinds the market would open now (`kinds_viable`), cafés and
    /// bookshops on dear streets too; picked by hash. `None`: nothing would
    /// pay its way here.
    fn shop_for_unit(&self, host: BuildingId, jobs: u16, salt: u64, role: UnitRole) -> Option<BusinessKind> {
        let h = self.buildings.get(host)?;
        let dear = self.lots.get(h.lot).is_some_and(|l| self.land_value.at(l.center) >= super::campus::DOWNTOWN_VALUE);
        let mall = h.plot.form.is_some_and(|f| f.is_mall());
        // [malls] Anchors are big stores, the food court cafés and bakeries;
        // a mall's shops are the whole menu (it is where the shops are).
        let menu: &[BusinessKind] = if role == UnitRole::Anchor {
            &[BusinessKind::Grocery, BusinessKind::GadgetShop]
        } else if role == UnitRole::FoodCourt {
            &[BusinessKind::Cafe, BusinessKind::Bakery]
        } else if dear || mall {
            &[BusinessKind::Grocery, BusinessKind::Bakery, BusinessKind::GadgetShop, BusinessKind::Cafe, BusinessKind::Bookshop]
        } else {
            &[BusinessKind::Grocery, BusinessKind::Bakery, BusinessKind::GadgetShop]
        };
        let viable = self.kinds_viable(menu, Some(jobs));
        if viable.is_empty() {
            return None;
        }
        let k = (hash01(h.seed, salt, 0x5409) * viable.len() as f32) as usize;
        viable.get(k.min(viable.len() - 1)).copied()
    }

    /// [tenants] Owners let empty units: a shopfront to a shop the market
    /// would open, more readily the more the city wants shops; upper floors
    /// to offices while the city is short of jobs. At most one unit a
    /// building and [`LETS_PER_DAY`] in all a day.
    pub(super) fn let_units(&mut self) {
        let day = self.clock.day();
        let demand = self.economy.demand_for(LandUse::Commercial).clamp(0.0, 1.0);
        let short_of_jobs = self.economy.job_gap();
        let hosts: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.vacant_units() > 0 && b.operating() && !b.is_derelict()).map(|(id, _)| id).collect();
        let (mut let_today, mut mall_today) = (0, 0);
        for host in hosts {
            if let_today >= LETS_PER_DAY && mall_today >= super::malls::MALL_LETS_PER_DAY {
                break;
            }
            // [malls] Malls let on their own allowance, up to two units a
            // day each, and more readily (shoppers come for the mall).
            let mall = self.buildings[host].plot.form.is_some_and(|f| f.is_mall());
            if (mall && mall_today >= super::malls::MALL_LETS_PER_DAY) || (!mall && let_today >= LETS_PER_DAY) {
                continue;
            }
            let vacant: Vec<usize> = self.buildings[host].units.iter().enumerate().filter(|(_, u)| u.vacant()).map(|(i, _)| i).take(if mall { 2 } else { 1 }).collect();
            for i in vacant {
                let h = &self.buildings[host];
                let seed = h.seed;
                let u = h.units[i];
                let pull = if mall { super::malls::MALL_LET_PULL } else { 1.0 };
                let chance = if u.shopfront { LET_CHANCE * (0.25 + demand) * pull } else { LET_CHANCE * (short_of_jobs * 10.0).clamp(0.0, 1.0) };
                if hash01(seed, day, i as u64) >= chance {
                    continue;
                }
                let kind = if u.shopfront { self.shop_for_unit(host, u.jobs, if mall { day ^ ((i as u64) << 20) } else { day }, u.role) } else { Some(BusinessKind::Office) };
                let Some(kind) = kind else { continue };
                let mut z = Business::new(kind, u.jobs);
                let what = match u.role {
                    UnitRole::Anchor => "the anchor store",
                    UnitRole::FoodCourt => "a food-court counter",
                    UnitRole::Shop if u.shopfront => "a shopfront",
                    UnitRole::Shop => "offices",
                };
                z.origin = Some(format!("Took {what} of {}, empty {} days.", self.buildings[host].name, self.clock.tick.saturating_sub(u.vacant_since) / 1440));
                if self.open_tenant(host, i, z, None).is_some() {
                    if mall {
                        mall_today += 1;
                    } else {
                        let_today += 1;
                    }
                    self.economy.market.tenants_let_total += 1;
                }
            }
        }
    }

    /// [tenants] A business that has outgrown its units (fully staffed,
    /// its trade paying its way, cash for the fit-out) takes the next empty
    /// unit of its kind in its building: a shop a shopfront, the rest a
    /// floor. One a building a day, [`GROWS_PER_DAY`] in all.
    pub(super) fn grow_into_units(&mut self) {
        let hosts: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.vacant_units() > 0 && b.operating()).map(|(id, _)| id).collect();
        let mut grown = 0;
        for host in hosts {
            if grown >= GROWS_PER_DAY {
                break;
            }
            let mut who: Vec<(Occupant, BuildingId)> = Vec::new();
            // [malls] A mall's own store keeps its one unit: the rest is let.
            if self.buildings[host].business.is_some() && !self.buildings[host].plot.form.is_some_and(|f| f.is_mall()) {
                who.push((Occupant::Main, host));
            }
            who.extend(self.tenants_of(host).into_iter().map(|t| (Occupant::Tenant(t), t)));
            for (occ, bid) in who {
                let b = &self.buildings[bid];
                let Some(z) = b.business.as_ref() else { continue };
                if z.closed || z.days_open < 3 || b.jobs == 0 || b.workers.len() < b.jobs as usize || self.economy.cover_of(z.kind).is_none_or(|c| c < super::develop::COVER_TO_EXPAND) {
                    continue;
                }
                let shop = fits_shopfront(z.kind);
                let money = z.money;
                let h = &self.buildings[host];
                let Some(i) = h.units.iter().position(|u| u.vacant() && u.shopfront == shop) else { continue };
                let cost = super::relocate::move_cost(h.units[i].jobs) * 0.5;
                if money < cost * 2.0 {
                    continue;
                }
                let jobs = h.units[i].jobs;
                self.buildings[host].units[i].occupant = occ;
                if occ == Occupant::Main {
                    self.buildings[host].recompute_capacity();
                } else {
                    self.buildings[bid].jobs += jobs;
                }
                self.transfer(Account::Business(bid), Account::Outside, cost, Kind::Building);
                self.events.push(SimEvent::BuildingChanged(host));
                grown += 1;
                self.economy.market.units_taken_total += 1;
                break;
            }
        }
    }

    /// [tenants] The building's own business shut for good while others
    /// rent units in it: its units are to let and the building stands
    /// (pulled down only when nobody is left in it).
    pub(super) fn vacate_main(&mut self, bid: BuildingId) {
        self.set_business(bid, None);
        let tick = self.clock.tick;
        if let Some(b) = self.buildings.get_mut(bid) {
            for u in b.units.iter_mut().filter(|u| u.occupant == Occupant::Main) {
                u.occupant = Occupant::Vacant;
                u.vacant_since = tick;
            }
            b.recompute_capacity();
        }
        self.events.push(SimEvent::BuildingChanged(bid));
    }

    /// [tenants] An empty unit within reach of `from` that a business of
    /// `kind` and about `jobs` jobs could move into (a shopfront for a
    /// shop, upper floors for the rest; at least half its jobs): the
    /// nearest, ties by key.
    pub(super) fn vacant_unit_near(&self, from: Vec2, kind: BusinessKind, jobs: u16, reach: f32) -> Option<(BuildingId, usize)> {
        let shop = fits_shopfront(kind);
        if !shop && kind != BusinessKind::Office {
            return None;
        }
        self.buildings
            .iter()
            .filter(|(_, b)| b.vacant_units() > 0 && b.operating() && !b.is_derelict())
            .filter_map(|(id, b)| {
                let i = b.units.iter().position(|u| u.vacant() && u.shopfront == shop && u.jobs * 2 >= jobs)?;
                let d = self.lots.get(b.lot)?.center.distance(from);
                (d <= reach).then_some((d, id, i))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, id, i)| (id, i))
    }
}
