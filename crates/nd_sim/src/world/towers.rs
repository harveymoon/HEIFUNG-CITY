//! [supertall] The skyscraper league on the `World` (`crate::towers`):
//! height limits (the city's, a district's, the clear approach round a
//! vertiport), the tower developers' money, where a tower may go and the
//! site it needs, starting a project (buying out the plots next door and
//! putting up a podium and tower over months), topping out (the record book
//! and the news), the skyline ranking, and the lift a landmark gives the
//! land round it.
//!
//! Costs: one pass over the lots and the buildings a day. Draws come from
//! hash-seeded generators (`erng`), never the world's own stream.

use super::*;
use crate::estate::{lots_touch, merged_lot_within, Form, OwnerId, OwnerKind};
use crate::land_value::LandValueBuilder;
use crate::services::ServiceKind;
use crate::towers::*;

/// What a project should be: everything optional (the dev command and the
/// tests force what is otherwise left to the market).
#[derive(Clone, Debug, Default)]
pub struct TowerOrder {
    /// Height to build (m); otherwise the developer decides.
    pub height: Option<f32>,
    pub design: Option<TowerDesign>,
    /// Build as near here as a site allows.
    pub near: Option<Vec2>,
    /// Skip the money, demand, population and downtown tests (the height
    /// limits and the site rules still hold).
    pub force: bool,
}

/// A place a tower could go: its primary plot and the score that ranks it.
#[derive(Clone, Debug)]
pub struct TowerSite {
    pub lot: LotId,
    pub score: f32,
    pub value: f32,
    pub neighbour_floors: u32,
    /// The limit there (m), if any.
    pub limit: Option<f32>,
}

impl World {
    // ── heights ────────────────────────────────────────────────────────────

    /// How tall a building stands (m): a league tower to its tip, anything
    /// else by its floors (the renderer's floor heights; roofs and masts
    /// aside).
    pub fn building_height(&self, bid: BuildingId) -> f32 {
        let Some(b) = self.buildings.get(bid) else { return 0.0 };
        if let Some(t) = &b.plot.tower {
            return t.height;
        }
        if b.land_use == LandUse::Park {
            return 0.0;
        }
        b.floors as f32 * FLOOR_M[b.land_use.index()]
    }

    /// Standing buildings, tallest first (ties by id): the skyline.
    /// Building sites, demolitions, parks, car parks and plants are left out.
    pub fn skyline_ranking(&self) -> Vec<(BuildingId, f32)> {
        let mut v: Vec<(BuildingId, f32)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.land_use != LandUse::Park && !b.is_parking() && !b.is_power())
            .map(|(id, _)| (id, self.building_height(id)))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// A standing building's place in the skyline (1 = the tallest).
    pub fn skyline_rank(&self, bid: BuildingId) -> Option<usize> {
        let h = self.building_height(bid);
        let b = self.buildings.get(bid)?;
        if !b.operating() {
            return None;
        }
        Some(1 + self.skyline_ranking().iter().take_while(|(id, x)| *x > h || (*x == h && *id < bid)).count())
    }

    /// The tallest standing building (m), 0 in an empty town.
    pub fn tallest_standing(&self) -> Option<(BuildingId, f32)> {
        self.skyline_ranking().first().copied()
    }

    /// The tallest standing or rising: what a record attempt has to beat.
    pub fn tallest_planned(&self) -> f32 {
        let rising = self.buildings.values().filter_map(|b| b.plot.tower.as_ref().filter(|_| b.demolition.as_ref().is_none_or(|d| !d.clear_lot)).map(|t| t.height)).fold(0.0, f32::max);
        rising.max(self.tallest_standing().map_or(0.0, |x| x.1))
    }

    /// League towers, tallest first, standing or rising.
    pub fn league_towers(&self) -> Vec<BuildingId> {
        let mut v: Vec<(BuildingId, f32)> = self.buildings.iter().filter_map(|(id, b)| b.plot.tower.map(|t| (id, t.height))).collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().map(|x| x.0).collect()
    }

    // ── the rules ──────────────────────────────────────────────────────────

    /// The height limit on a lot (m): the city's, the district's, and the
    /// clear approach round the player's vertiports, the lowest of them;
    /// `None` for no limit.
    pub fn height_limit_at(&self, lot: LotId) -> Option<f32> {
        let l = self.lots.get(lot)?;
        let mut lim = self.skyline.limit;
        if let Some(d) = self.districts.at(l.center).and_then(|d| d.height_limit) {
            lim = Some(lim.map_or(d, |x| x.min(d)));
        }
        if self.skyline.vertiport_clearance {
            let near_pad = self.buildings.values().any(|b| b.service.is_some_and(|s| s.kind == ServiceKind::Vertiport) && self.lots.get(b.lot).is_some_and(|p| p.center.distance(l.center) < VERTIPORT_CLEARANCE_M));
            if near_pad {
                lim = Some(lim.map_or(VERTIPORT_CLEARANCE_CAP_M, |x| x.min(VERTIPORT_CLEARANCE_CAP_M)));
            }
        }
        lim
    }

    /// The city-wide height limit (m), or none. Standing towers stay; it
    /// holds what is planned from now on.
    pub fn set_height_limit(&mut self, limit: Option<f32>) {
        self.skyline.limit = limit.filter(|m| m.is_finite()).map(|m| m.clamp(10.0, TOWER_MAX_M));
    }

    // ── the day ────────────────────────────────────────────────────────────

    /// Once a day after the land market: the tower developers' money grows
    /// with the city, and maybe one of them breaks ground.
    pub(super) fn daily_towers(&mut self) {
        let pop = self.citizens.len();
        let gate = self.skyline.always || pop >= TOWER_MIN_POP;
        // Owner records the estate dropped (owners of nothing) are made
        // again with the next project.
        let owners = &self.estate.owners;
        for d in &mut self.skyline.developers {
            if d.owner.is_some_and(|o| !owners.contains_key(o)) {
                d.owner = None;
            }
        }
        if !gate {
            return;
        }
        // Investors follow a growing city with demand for space.
        let want = (1 + pop / 5000).min(4);
        while self.skyline.developers.len() < want {
            self.new_tower_developer();
        }
        let demand = self.tower_demand();
        let inflow = pop.max(TOWER_MIN_POP) as f64 * CAPITAL_PER_PERSON * (0.4 + demand.clamp(0.0, 1.0) as f64);
        let cap = structure_cost(TOWER_MAX_M) * 1.5;
        // [ledger] Investors' money from out of town, up to the cap.
        for i in 0..self.skyline.developers.len() {
            let d = &self.skyline.developers[i];
            let add = ((d.capital + inflow * d.pull as f64).min(cap) - d.capital).max(0.0);
            self.transfer64(Account::Outside, Account::Tower(i as u16), add, Kind::Investment);
        }
        let day = self.clock.day() as u32;
        if self.skyline.last_start_day.is_some_and(|d| day < d + TOWER_START_GAP_DAYS) {
            return;
        }
        // Projects at once: one, and another per 10,000 people (three at most).
        let rising = self.buildings.values().filter(|b| b.plot.tower.is_some() && b.construction.is_some()).count();
        if rising >= (1 + pop / 10_000).min(3) {
            return;
        }
        if demand < TOWER_MIN_DEMAND {
            return;
        }
        let mut rng = self.erng(0x70_3E5);
        if !rng.random_bool(TOWER_DAILY_CHANCE) {
            return;
        }
        let _ = self.start_tower(TowerOrder::default());
    }

    /// Demand for tower space: the stronger of offices' (commercial) and
    /// homes'.
    pub fn tower_demand(&self) -> f32 {
        self.economy.demand_for(LandUse::Commercial).max(self.economy.demand_for(LandUse::Residential))
    }

    fn new_tower_developer(&mut self) -> usize {
        let n = self.skyline.developers.len() as u64;
        let taken: Vec<String> = self.skyline.developers.iter().map(|d| d.name.clone()).chain(self.estate.owners.values().map(|o| o.name.clone())).collect();
        let mut k = 0u64;
        let name = loop {
            let h = self.config.seed ^ 0xDE7E_10 ^ (n * 7919 + k).wrapping_mul(0x9E37_79B9);
            let name = developer_name(crate::towers::unit(h).to_bits() as u64 ^ h.rotate_left(17));
            if !taken.contains(&name) || k > 40 {
                break name;
            }
            k += 1;
        };
        let pull = 0.7 + 0.6 * unit(self.config.seed ^ (n + 1).wrapping_mul(0x51ED));
        self.skyline.developers.push(TowerDeveloper { name, owner: None, capital: 0.0, pull });
        self.skyline.developers.len() - 1
    }

    // ── where ──────────────────────────────────────────────────────────────

    /// The floors round a tower site that count (`TOWER_DENSITY_RADIUS`):
    /// the grown buildings standing open, no parks. Sorted into a grid
    /// once for a whole `tower_sites` pass.
    fn neighbour_floors_grid(&self) -> super::floors_grid::FloorsGrid {
        let items = self
            .buildings
            .values()
            .filter(|b| b.land_use != LandUse::Park && b.is_grown() && b.operating())
            .filter_map(|b| self.lots.get(b.lot).map(|l| (b.lot, l.center, b.floors as u32)));
        super::floors_grid::FloorsGrid::new(TOWER_DENSITY_RADIUS, items)
    }

    /// May the market build on this plot as part of a tower site? Vacant,
    /// or a grown building it may buy (never the city's, a home bought
    /// outright, a billionaire's or another tower), not under a facility,
    /// not promised to anyone, not in a historic district.
    fn tower_plot_ok(&self, lid: LotId, lu: LandUse) -> bool {
        let Some(l) = self.lots.get(lid) else { return false };
        if l.land_use != lu || !self.site_clear(lid, SiteRule::ANY_NOT_HISTORIC) {
            return false;
        }
        match l.building {
            None => true,
            Some(b) => self.buyable(b) && self.buildings.get(b).is_some_and(|x| !x.plot.merged() && x.plot.form.is_none() && x.business.as_ref().is_none_or(|z| !matches!(z.kind, BusinessKind::TaxiCompany | BusinessKind::CargoHub))),
        }
    }

    /// Where a tower could go, best first: offices or homes on the dearest,
    /// densest land (unless `force`), with room under the height limit for
    /// a skyscraper at least.
    pub fn tower_sites(&self, force: bool) -> Vec<TowerSite> {
        let pressure = self.pressure_active();
        let mut out: Vec<TowerSite> = Vec::new();
        let towers: Vec<Vec2> = self.buildings.values().filter(|b| b.plot.tower.is_some()).filter_map(|b| self.lots.get(b.lot).map(|l| l.center)).collect();
        let mut grid = None;
        for (lid, l) in self.lots.iter() {
            if !matches!(l.land_use, LandUse::Commercial | LandUse::Residential) || !l.usable() || !l.car_access() || l.landlocked {
                continue;
            }
            if !self.tower_plot_ok(lid, l.land_use) {
                continue;
            }
            let value = self.lot_value(lid);
            if !force && value < TOWER_MIN_VALUE {
                continue;
            }
            let pf = if pressure { self.pressure_floors(lid) } else { PRESSURE_MAX_FLOORS_SITE };
            if !force && pf < TOWER_MIN_PRESSURE_FLOORS {
                continue;
            }
            let limit = self.height_limit_at(lid);
            if limit.is_some_and(|m| m < SKYSCRAPER_M) {
                continue;
            }
            // Not on top of another tower.
            if towers.iter().any(|t| t.distance(l.center) < 45.0) {
                continue;
            }
            let nf = grid.get_or_insert_with(|| self.neighbour_floors_grid()).floors_within(l.center, TOWER_DENSITY_RADIUS, lid);
            if !force && nf < TOWER_MIN_NEIGHBOUR_FLOORS {
                continue;
            }
            let score = value * (pf.min(40) as f32 + nf as f32 / 12.0) * if l.land_use == LandUse::Commercial { 1.15 } else { 1.0 };
            out.push(TowerSite { lot: lid, score, value, neighbour_floors: nf, limit });
        }
        out.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.lot.cmp(&b.lot)));
        out
    }

    /// The plots a tower of `league` on `primary` would stand on: the
    /// primary and touching plots of its land use (vacant first, then the
    /// cheapest), until the site is big enough, as long as they make one
    /// compact block. `None` if no such site can be put together.
    pub fn assemble_tower_site(&self, primary: LotId, league: League) -> Option<Vec<LotId>> {
        let p = self.lots.get(primary)?;
        let lu = p.land_use;
        let mut ours = vec![primary];
        let mut area = p.area();
        while ours.len() < league.min_lots() || area < league.min_area() {
            if ours.len() >= TOWER_MAX_LOTS {
                return None;
            }
            let mine: Vec<&Lot> = ours.iter().filter_map(|l| self.lots.get(*l)).collect();
            let mut cands: Vec<(f64, LotId)> = Vec::new();
            for (lid, l) in self.lots.iter() {
                if ours.contains(&lid) || !mine.iter().any(|m| m.center.distance(l.center) < 140.0 && lots_touch(m, l)) || !self.tower_plot_ok(lid, lu) {
                    continue;
                }
                let cost = l.building.map_or(0.0, |b| self.sale_price(b));
                cands.push((cost, lid));
            }
            cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let mut added = false;
            for (_, lid) in cands {
                let others: Vec<&Lot> = ours.iter().skip(1).chain(std::iter::once(&lid)).filter_map(|l| self.lots.get(*l)).collect();
                if merged_lot_within(p, &others, TOWER_HULL_SLACK).is_none() {
                    continue;
                }
                area += self.lots[lid].area();
                ours.push(lid);
                added = true;
                break;
            }
            if !added {
                return None;
            }
        }
        Some(ours)
    }

    // ── building one ───────────────────────────────────────────────────────

    /// A developer breaks ground on a league tower: picks the height (over
    /// the record when demand and money allow, under every limit), the
    /// best site that can be assembled for it, buys the plots (the grown
    /// buildings on them come down; residents move out with a memory) and
    /// starts a long build. Returns the tower's building.
    pub fn start_tower(&mut self, order: TowerOrder) -> Result<BuildingId, String> {
        let pop = self.citizens.len();
        if !order.force && !self.skyline.always && pop < TOWER_MIN_POP {
            return Err(format!("the city needs {TOWER_MIN_POP} people for a tower (has {pop})"));
        }
        if self.skyline.developers.is_empty() {
            self.new_tower_developer();
        }
        // The richest developer moves first.
        let dev = (0..self.skyline.developers.len()).max_by(|a, b| self.skyline.developers[*a].capital.total_cmp(&self.skyline.developers[*b].capital).then(b.cmp(a))).unwrap();
        let capital = self.skyline.developers[dev].capital;
        let demand = self.tower_demand();
        let record = self.tallest_planned();
        let pop_cap = if order.force || self.skyline.always { TOWER_MAX_M } else { height_for_population(pop) };
        let salt = self.skyline.projects_total as u64;
        let roll = unit(self.config.seed ^ 0x5EED_70 ^ salt.wrapping_mul(0x9E37) ^ self.clock.tick);
        let mut sites = self.tower_sites(order.force);
        if let Some(p) = order.near {
            sites.sort_by(|a, b| {
                let da = self.lots[a.lot].center.distance(p);
                let db = self.lots[b.lot].center.distance(p);
                da.total_cmp(&db).then(a.lot.cmp(&b.lot))
            });
        }
        if sites.is_empty() {
            return Err("no site: towers need dense, dear downtown land (commercial or residential, land value 0.6+, busy streets, tall neighbours)".into());
        }
        let mut why = String::from("no site could be assembled");
        for site in sites.iter().take(40) {
            let limit = site.limit.unwrap_or(TOWER_MAX_M).min(pop_cap);
            // Land: roughly what the plots will cost (the primary's own
            // building and its neighbours), for the money test.
            let affordable = if order.force { TOWER_MAX_M } else { affordable_height(capital * 0.8) };
            let (height, record_bid) = match order.height {
                Some(h) => (h.clamp(SKYSCRAPER_M, TOWER_MAX_M), h > record),
                None => {
                    let step = RECORD_STEP.0 + (RECORD_STEP.1 - RECORD_STEP.0) * roll;
                    let beat = (record * step).max(SKYSCRAPER_M + 30.0 * roll);
                    if beat <= limit && beat <= affordable && (demand >= RECORD_MIN_DEMAND || order.force) {
                        (beat, true)
                    } else {
                        // A tower that does not chase the record: somewhere
                        // in the skyscraper class under the record.
                        let top = limit.min(affordable).min(record.max(SKYSCRAPER_M + 40.0) * 0.92);
                        if top < SKYSCRAPER_M {
                            why = if limit < SKYSCRAPER_M { "the height limit holds every site under 150 m".into() } else { format!("no developer can afford a skyscraper yet (richest has ${:.0}M)", capital / 1e6) };
                            continue;
                        }
                        (SKYSCRAPER_M + (top - SKYSCRAPER_M) * unit(salt ^ 0xA11), false)
                    }
                }
            };
            if height > limit + 0.01 {
                why = format!("the height limit ({limit:.0} m) is under {height:.0} m");
                continue;
            }
            let league = League::for_height(height).unwrap_or(League::Skyscraper);
            let Some(plots) = self.assemble_tower_site(site.lot, league) else {
                why = format!("no {} site could be assembled", league.label());
                continue;
            };
            let price: f64 = self.plots_price(&plots);
            let cost = structure_cost(height) + price;
            if !order.force && cost > capital {
                why = format!("the richest developer has ${:.0}M of the ${:.0}M it would take", capital / 1e6, cost / 1e6);
                continue;
            }
            let bid = self.break_ground(dev, &plots, height, order.design, record_bid && height > record, price);
            // [ledger] The plots and the build, paid out of town (what a
            // forced order cannot afford, the bank covers).
            let pay = cost.min(self.skyline.developers[dev].capital.max(0.0));
            self.transfer64(Account::Tower(dev as u16), Account::Outside, pay, Kind::Building);
            return Ok(bid);
        }
        Err(why)
    }

    /// The developer's owner record, made on first use.
    fn tower_owner(&mut self, dev: usize) -> OwnerId {
        if let Some(o) = self.skyline.developers[dev].owner.filter(|o| self.estate.owners.contains_key(*o)) {
            return o;
        }
        let name = self.skyline.developers[dev].name.clone();
        let o = self.new_owner(name, OwnerKind::Developer);
        self.skyline.developers[dev].owner = Some(o);
        o
    }

    fn break_ground(&mut self, dev: usize, plots: &[LotId], height: f32, design: Option<TowerDesign>, record_bid: bool, price: f64) -> BuildingId {
        let primary = plots[0];
        let owner = self.tower_owner(dev);
        let dev_name = self.skyline.developers[dev].name.clone();
        let seed = self.fresh_seed();
        let name = tower_name(seed ^ 0x70E2);
        let lu = self.lots[primary].land_use;
        let value = self.lot_value(primary);
        let day = self.clock.day() as u32;
        let tower = Tower { height, design: design.unwrap_or_else(|| TowerDesign::for_seed(seed, height)), seed, started_day: day, topped_out_day: None, record_bid };
        let floors = tower.floors();
        let memory = format!("Our block was bought by {dev_name} for {name}. We had to go.");
        let bid = match self.lots[primary].building {
            Some(b) => {
                // [review] Its owner is paid and its business moves out
                // first, as in every other acquisition (both were lost).
                self.pack_business(b, &name);
                self.settle_sale(b);
                self.redevelop_with(b, Tier::High, floors, value, memory.clone(), false);
                self.tag_demolition(b, crate::buildings::DemolitionReason::Landmark);
                let bld = &mut self.buildings[b];
                bld.land_use = lu;
                bld.look = None;
                b
            }
            None => {
                let mut b = Building::new(primary, lu, floors, Tier::High, seed, name.clone(), self.clock.tick, value);
                b.construction = Some(Construction::new(1.0));
                b.recompute_capacity();
                b.update_rent(value);
                let id = self.buildings.insert(b);
                self.lots[primary].building = Some(id);
                self.events.push(SimEvent::BuildingBuilt(id));
                self.order_materials(id);
                id
            }
        };
        {
            let b = &mut self.buildings[bid];
            b.name = name.clone();
            b.plot.form = Some(Form::Tower);
            b.plot.tower = Some(tower);
            b.high_land_days = 0;
            if let Some(c) = b.construction.as_mut() {
                c.days = build_days(height);
            }
            b.business = (lu == LandUse::Commercial).then(|| Business::new(BusinessKind::Office, 0));
            b.recompute_capacity();
            b.update_rent(value);
        }
        if lu == LandUse::Commercial {
            let jobs = self.buildings[bid].planned_capacity().1;
            self.set_business(bid, Some(Business::new(BusinessKind::Office, jobs)));
        }
        let rest: Vec<LotId> = plots[1..].to_vec();
        self.take_plots_for(bid, &rest, &memory, crate::buildings::DemolitionReason::Landmark);
        self.give_to(bid, owner, price);
        self.skyline.projects_total += 1;
        self.skyline.plots_bought_total += plots.iter().filter(|l| **l != primary).count() as u32;
        self.skyline.last_start_day = Some(day);
        let pos = self.lots[primary].center;
        let headline = if record_bid {
            format!("{dev_name} breaks ground on {name}: {height:.0} m planned, set to be the city's tallest")
        } else {
            format!("{dev_name} breaks ground on {name}, a {height:.0} m {}", tower.league().label())
        };
        let tick = self.clock.tick;
        self.skyline.push_news(tick, headline, pos, Some(bid));
        self.events.push(SimEvent::BuildingChanged(bid));
        self.events.push(SimEvent::LotsChanged);
        bid
    }

    /// A league tower topped out: its day, the record book if it is the
    /// tallest, and a line of news either way.
    pub(super) fn tower_finished(&mut self, bid: BuildingId) {
        let Some(b) = self.buildings.get(bid) else { return };
        let Some(t) = b.plot.tower else { return };
        if t.topped_out_day.is_some() {
            return;
        }
        let day = self.clock.day() as u32;
        let name = b.name.clone();
        let dev = self.owner_of(bid).map(|(_, o)| o.name.clone()).unwrap_or_else(|| "A developer".into());
        let pos = self.footprint(bid).map(|l| l.center).unwrap_or_default();
        if let Some(t) = self.buildings[bid].plot.tower.as_mut() {
            t.topped_out_day = Some(day);
        }
        self.skyline.topped_total += 1;
        let rank = self.skyline_rank(bid).unwrap_or(1);
        let tallest_other = self.skyline_ranking().into_iter().filter(|(id, _)| *id != bid).map(|x| x.1).fold(0.0, f32::max);
        let headline = if rank == 1 && t.height > tallest_other {
            self.skyline.records.push(RecordEntry { day, name: name.clone(), developer: dev.clone(), height: t.height, building: Some(bid) });
            if self.skyline.records.len() > 60 {
                self.skyline.records.remove(0);
            }
            format!("{dev} tops out {name} at {:.0} m, the city's tallest", t.height)
        } else {
            format!("{dev} tops out {name} at {:.0} m, the city's {} tallest", t.height, ordinal(rank))
        };
        let tick = self.clock.tick;
        self.skyline.push_news(tick, headline, pos, Some(bid));
    }

    /// Land value: an address by a landmark tower is worth more, the
    /// taller the tower the further it reaches. Called from the land
    /// value's strip 3.
    pub(super) fn splat_towers(&self, b: &mut LandValueBuilder) {
        for (id, bld) in self.buildings.iter() {
            let Some(t) = &bld.plot.tower else { continue };
            if !bld.operating() {
                continue;
            }
            let Some(c) = self.footprint(id).map(|l| l.center) else { continue };
            let r = (140.0 + t.height * 0.4).min(460.0);
            let w = (0.04 + 0.08 * (t.height / MEGATALL_M)).min(0.14);
            b.splat(c, r, w);
        }
    }

    /// Finish every tower site at once (dev): demolitions cleared, the
    /// plots joined, the towers topped out.
    pub fn finish_towers(&mut self) -> usize {
        let ids: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.plot.tower.is_some() && b.construction.is_some()).map(|(id, _)| id).collect();
        for &id in &ids {
            let pending = self.buildings[id].plot.pending_lots.clone();
            for l in pending {
                if let Some(other) = self.lots.get(l).and_then(|x| x.building).filter(|o| *o != id) {
                    if self.buildings.get(other).is_some_and(|o| o.demolition.is_some()) {
                        self.finish_demolition(other);
                    }
                }
                self.claim_reserved(l);
            }
            self.finish_construction(id);
        }
        ids.len()
    }
}

/// Pressure floors to assume where pressure does not cap heights yet (a
/// small or new town): enough for a tower.
const PRESSURE_MAX_FLOORS_SITE: u8 = 40;
