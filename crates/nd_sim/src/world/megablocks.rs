//! [megablocks] Mega-blocks on the `World` (`crate::megablock`): a
//! mega-corp's flagship arcology. Once a corp is big and the town can fill
//! it, it puts together a block of plots round land it already holds (its
//! own buildings, vacant plots, low buildings the market may buy), pulls
//! down what stands there and builds the block over months in four stages.
//! Open, it lets its street halls, market hall and offices (`let_units`,
//! on an allowance of its own); its residents:
//! - go to a job, a shop or a bar inside without setting foot on the street
//!   (`trip_inside_megablock`: a lift ride, never routed over the roads),
//! - take a free job inside before one outside (`megablock_job_for`, and up
//!   to [`JOB_MOVES_PER_DAY`] who work outside swap a day),
//! - go up to a sky garden instead of out to a park (`sky_garden_visit`).
//!
//! Round it: the land is worth more out to [`LIFT_RADIUS`], shaded close in
//! ([`SHADE_RADIUS`]); the corp's brand draws custom to its shops city-wide
//! (`brand_pull`); its car decks and deliveries are the street's traffic.
//!
//! Costs: a pass over the blocks a day; the site search only on the day a
//! corp tries. No draws from the world's stream but the new building's
//! seed (`fresh_seed`, a counter): every pick is a hash.

use super::*;
use crate::estate::{lots_touch, merged_lot_within, Form, OwnerId, OwnerKind};
use crate::hash::hash01;
use crate::megablock::*;

/// What the inspector and the API show of one mega-block.
#[derive(Clone, Debug, Default)]
pub struct MegaBlockReport {
    pub name: String,
    pub corp: String,
    pub open: bool,
    pub progress: f32,
    pub stage: Option<&'static str>,
    pub days_left: f32,
    pub plots: usize,
    pub area_m2: f32,
    pub towers: u8,
    pub floors: u8,
    pub height_m: f32,
    pub homes: u16,
    pub households: usize,
    pub residents: usize,
    pub shops: usize,
    pub shops_let: usize,
    pub market_counters: usize,
    pub counters_let: usize,
    pub offices: usize,
    pub offices_let: usize,
    pub tenants: Vec<(BuildingId, String, &'static str)>,
    pub jobs: u32,
    pub jobs_filled: u32,
    /// Residents working inside, and residents working at all.
    pub work_inside: usize,
    pub workers: usize,
    pub gardens: u32,
    pub bridges: u32,
    pub parking: u16,
    pub inside_yesterday: u32,
    pub out_yesterday: u32,
    pub gardens_yesterday: u32,
    pub inside_today: u32,
    pub out_today: u32,
    pub gardens_today: u32,
    pub inside_total: u64,
}

impl World {
    // ── reading ───────────────────────────────────────────────────────────

    /// [megablocks] The open mega-block `bid` is in: itself, or the block
    /// a tenant rents in.
    pub fn megablock_of(&self, bid: BuildingId) -> Option<BuildingId> {
        let h = self.host_of(bid);
        self.buildings.get(h).filter(|b| b.plot.megablock.is_some() && b.plot.form == Some(Form::MegaBlock) && b.operating()).map(|_| h)
    }

    /// [megablocks] Every mega-block, rising or standing, in id order.
    pub fn megablock_list(&self) -> Vec<BuildingId> {
        self.buildings.iter().filter(|(_, b)| b.plot.megablock.is_some() && b.plot.form == Some(Form::MegaBlock)).map(|(id, _)| id).collect()
    }

    /// [megablocks] Owners whose brand has a standing mega-block: their
    /// shops pull custom (`brand_pull`).
    pub fn megablock_brands(&self) -> Vec<OwnerId> {
        self.corps.corps.iter().filter(|c| c.megablock.is_some_and(|b| self.megablock_of(b) == Some(b))).map(|c| c.owner).collect()
    }

    /// [megablocks] What the corp's brand adds to a shop's or a place's
    /// pull: its own (by its building's owner) and every one in its block.
    pub fn brand_pull(&self, bid: BuildingId, brands: &[OwnerId]) -> f32 {
        if brands.is_empty() {
            return 0.0;
        }
        let h = self.host_of(bid);
        let owned = self.buildings.get(h).and_then(|b| b.plot.owner).is_some_and(|o| brands.contains(&o));
        if owned { BRAND_PULL } else { 0.0 }
    }

    /// [megablocks] The report of the mega-block `bid` (rising or open).
    pub fn megablock_report(&self, bid: BuildingId) -> Option<MegaBlockReport> {
        let b = self.buildings.get(bid)?;
        let m = b.plot.megablock?;
        let open = b.operating();
        let corp = self.estate.owners.get(m.corp).map_or_else(String::new, |o| o.name.clone());
        let progress = b.construction.as_ref().map_or(1.0, |c| c.progress);
        let mut r = MegaBlockReport {
            name: b.name.clone(),
            corp,
            open,
            progress,
            stage: (!open).then(|| Stage::of_progress(progress).label()),
            days_left: b.construction.as_ref().map_or(0.0, |c| c.days_left()),
            plots: 1 + b.plot.extra_lots.len() + b.plot.pending_lots.len(),
            area_m2: b.plot.lots(b.lot).iter().chain(b.plot.pending_lots.iter()).filter_map(|l| self.lots.get(*l)).map(|l| l.area()).sum(),
            towers: m.towers,
            floors: m.total_floors(),
            height_m: self.building_height(bid),
            homes: m.homes,
            households: b.households.len(),
            residents: b.residents.len(),
            gardens: m.gardens(),
            bridges: m.bridges(),
            parking: b.parking_capacity,
            inside_yesterday: m.inside_yesterday,
            out_yesterday: m.out_yesterday,
            gardens_yesterday: m.gardens_yesterday,
            inside_today: m.inside_today,
            out_today: m.out_today,
            gardens_today: m.gardens_today,
            inside_total: m.inside_total,
            ..MegaBlockReport::default()
        };
        for u in &b.units {
            let let_ = !u.vacant();
            match (u.shopfront, u.role) {
                (true, crate::buildings::UnitRole::FoodCourt) => {
                    r.market_counters += 1;
                    r.counters_let += let_ as usize;
                }
                (true, _) => {
                    r.shops += 1;
                    r.shops_let += let_ as usize;
                }
                (false, _) => {
                    r.offices += 1;
                    r.offices_let += let_ as usize;
                }
            }
        }
        if !open {
            r.shops = m.shops as usize;
            r.market_counters = MARKET_COUNTERS;
            r.offices = m.offices as usize;
        }
        for t in self.tenants_of(bid) {
            let Some(tb) = self.buildings.get(t) else { continue };
            r.jobs += tb.jobs as u32;
            r.jobs_filled += tb.workers.len() as u32;
            r.tenants.push((t, tb.name.clone(), tb.business.as_ref().map_or("unit", |z| z.kind.label())));
        }
        for c in b.residents.iter().filter_map(|c| self.citizens.get(*c)) {
            if let Some(w) = c.work {
                r.workers += 1;
                if self.megablock_of(w) == Some(bid) {
                    r.work_inside += 1;
                }
            }
        }
        Some(r)
    }

    // ── the site ──────────────────────────────────────────────────────────

    /// [megablocks] May corp `corp`'s block take this plot? Homes or
    /// commercial land, not under a raised road or in a frozen district;
    /// vacant, or a low ordinary building the market may buy or the corp
    /// itself owns (not its HQ).
    pub(super) fn megablock_plot_ok(&self, lid: LotId, corp: OwnerId, hq: Option<BuildingId>) -> bool {
        let Some(l) = self.lots.get(lid) else { return false };
        if !matches!(l.land_use, LandUse::Residential | LandUse::Commercial | LandUse::Entertainment) || !l.usable() || !self.site_clear(lid, SiteRule::ANY_NOT_HISTORIC) || self.under_a_deck(lid) {
            return false;
        }
        match l.building {
            None => true,
            Some(b) => {
                let Some(x) = self.buildings.get(b) else { return false };
                let ordinary = x.operating() && x.is_grown() && !x.is_special() && !x.plot.merged() && x.plot.form.is_none() && x.plot.tower.is_none() && x.floors <= MEGABLOCK_BUY_MAX_FLOORS && x.plot.pending_lots.is_empty();
                ordinary && Some(b) != hq && (self.buyable(b) || x.plot.owner == Some(corp)) && x.business.as_ref().is_none_or(|z| !matches!(z.kind, BusinessKind::TaxiCompany | BusinessKind::CargoHub))
            }
        }
    }

    /// [megablocks] The plots a block on `primary` would stand on: every
    /// plot whose middle falls in a rectangle round the primary (squared to
    /// its street front, a few sizes and placements tried), all of them
    /// plots the block may take (no hole of someone else's in it), making
    /// one compact block of at least [`MEGABLOCK_MIN_AREA`]. Of those, the
    /// one nearest [`MEGABLOCK_TARGET_AREA`] and cheapest. `None` if none.
    pub fn megablock_site(&self, primary: LotId, corp: OwnerId) -> Option<Vec<LotId>> {
        self.megablock_site_why(primary, corp, &mut [0; 6])
    }

    /// [megablocks] `megablock_site`, counting why each try failed: the
    /// primary itself, a hole, too many plots, too small, not compact
    /// (hull), the wrong shape.
    pub fn megablock_site_why(&self, primary: LotId, corp: OwnerId, why: &mut [u32; 6]) -> Option<Vec<LotId>> {
        let hq = self.corps.corp_of_owner(corp).and_then(|c| c.hq);
        let p = self.lots.get(primary)?;
        if p.landlocked || !p.car_access() || !self.megablock_plot_ok(primary, corp, hq) {
            why[0] += 1;
            return None;
        }
        let along = Vec2::new(-p.front_dir.y, p.front_dir.x);
        let local = |q: Vec2| Vec2::new((q - p.center).dot(along), (q - p.center).dot(p.front_dir));
        // Every plot near by, where it is and whether the block may take it.
        let near: Vec<(LotId, Vec2, bool)> = self
            .lots
            .iter()
            .filter(|(id, l)| *id != primary && l.center.distance(p.center) < 240.0)
            .map(|(id, l)| (id, local(l.center), self.megablock_plot_ok(id, corp, hq)))
            .collect();
        let cost_of = |l: LotId| self.lots.get(l).and_then(|x| x.building).map_or(0.0, |b| if self.buildings.get(b).is_some_and(|x| x.plot.owner == Some(corp)) { 0.0 } else { self.sale_price(b) as f32 });
        let mut best: Option<(f32, Vec<LotId>)> = None;
        for (w, d) in [(96.0f32, 96.0f32), (110.0, 80.0), (80.0, 110.0), (84.0, 70.0), (70.0, 84.0), (120.0, 64.0), (64.0, 120.0), (76.0, 76.0), (68.0, 68.0), (90.0, 58.0), (58.0, 90.0), (62.0, 76.0), (76.0, 62.0)] {
            for fu in [0.1f32, 0.3, 0.5, 0.7, 0.9] {
                for fv in [0.05f32, 0.3, 0.5, 0.7, 0.95] {
                    // The box in the primary's frame: u across its front, v
                    // into the block (the street is at v < 0).
                    let (u0, v0) = (-w * fu, -d * fv);
                    let inside = |q: Vec2| q.x >= u0 && q.x <= u0 + w && q.y >= v0 && q.y <= v0 + d;
                    if near.iter().any(|(_, q, ok)| !ok && inside(*q)) {
                        why[1] += 1;
                        continue;
                    }
                    let mut site: Vec<LotId> = vec![primary];
                    site.extend(near.iter().filter(|(_, q, _)| inside(*q)).map(|(id, _, _)| *id));
                    if site.len() > MEGABLOCK_MAX_LOTS {
                        why[2] += 1;
                        continue;
                    }
                    let mut area: f32 = site.iter().filter_map(|l| self.lots.get(*l)).map(|l| l.area()).sum();
                    if area < MEGABLOCK_MIN_AREA {
                        why[3] += 1;
                        continue;
                    }
                    let rest: Vec<&Lot> = site.iter().skip(1).filter_map(|x| self.lots.get(*x)).collect();
                    let Some(hull) = merged_lot_within(p, &rest, MEGABLOCK_HULL_SLACK) else {
                        why[4] += 1;
                        continue;
                    };
                    // Nothing of anyone else's left standing inside the
                    // podium: a plot whose middle the block's outline takes
                    // in is the block's too, or the try fails.
                    let caught: Vec<(LotId, bool)> = near.iter().filter(|(id, _, _)| !site.contains(id)).filter(|(id, _, _)| self.lots.get(*id).is_some_and(|l| crate::zoning::point_in_polygon(l.center, &hull.polygon))).map(|(id, _, ok)| (*id, *ok)).collect();
                    if caught.iter().any(|(_, ok)| !ok) || site.len() + caught.len() > MEGABLOCK_MAX_LOTS {
                        why[1] += 1;
                        continue;
                    }
                    site.extend(caught.iter().map(|x| x.0));
                    area += caught.iter().filter_map(|x| self.lots.get(x.0)).map(|l| l.area()).sum::<f32>();
                    let (sw, sd) = self.site_extent(primary, &site[1..]);
                    if sw.min(sd) < MEGABLOCK_MIN_SIDE || sw.min(sd) < sw.max(sd) * MEGABLOCK_MIN_ASPECT {
                        why[5] += 1;
                        continue;
                    }
                    let cost: f32 = site.iter().map(|l| cost_of(*l)).sum();
                    let score = (area - MEGABLOCK_TARGET_AREA).abs() + cost / 2_000.0;
                    if best.as_ref().is_none_or(|(s, _)| score < *s - 1e-3) {
                        best = Some((score, site));
                    }
                }
            }
        }
        let _ = lots_touch;
        best.map(|x| x.1)
    }

    /// [megablocks] Where corp `ci` would start looking: the plots of its
    /// own homes and commercial holdings (land it has assembled), nearest
    /// its HQ first; then the plots round the HQ.
    pub fn megablock_primaries(&self, ci: usize) -> Vec<LotId> {
        let c = &self.corps.corps[ci];
        let origin = c.hq.or(c.flagship).and_then(|b| self.building_lot(b)).map(|l| l.center).unwrap_or_default();
        let props = self.estate.owners.get(c.owner).map(|o| o.properties.clone()).unwrap_or_default();
        let mut own: Vec<(f32, LotId)> = props
            .iter()
            .filter(|p| Some(**p) != c.hq)
            .filter_map(|p| self.buildings.get(*p))
            .filter(|b| matches!(b.land_use, LandUse::Residential | LandUse::Commercial))
            .filter_map(|b| self.lots.get(b.lot).map(|l| (l.center.distance(origin), b.lot)))
            .collect();
        own.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut round: Vec<(f32, LotId)> = self.lots.iter().filter(|(_, l)| matches!(l.land_use, LandUse::Residential | LandUse::Commercial) && !l.landlocked && l.car_access()).map(|(id, l)| (l.center.distance(origin), id)).filter(|(d, _)| *d < 900.0).collect();
        round.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        own.into_iter().chain(round.into_iter().step_by(3)).map(|x| x.1).collect()
    }

    /// [megablocks] Why corp `ci` may not start a mega-block now (`None`:
    /// it may).
    pub fn megablock_refusal(&self, ci: usize) -> Option<String> {
        let c = self.corps.corps.get(ci)?;
        if c.megablock.is_some_and(|b| self.buildings.contains_key(b)) {
            return Some(format!("{} has its mega-block already", c.brand.name));
        }
        let pop = self.citizens.len();
        if pop < MEGABLOCK_MIN_POP {
            return Some(format!("the town needs {MEGABLOCK_MIN_POP} people for a mega-block (has {pop})"));
        }
        let age = (self.clock.day() as u32).saturating_sub(c.founded_day);
        if age < MEGABLOCK_CORP_MIN_DAYS {
            return Some(format!("{} has been a corp {age} days (needs {MEGABLOCK_CORP_MIN_DAYS})", c.brand.name));
        }
        let held = self.estate.owners.get(c.owner).map_or(0, |o| o.properties.len());
        if held < MEGABLOCK_MIN_HOLDINGS {
            return Some(format!("{} holds {held} properties (needs {MEGABLOCK_MIN_HOLDINGS})", c.brand.name));
        }
        if self.buildings.values().any(|b| b.plot.megablock.is_some() && b.construction.is_some()) {
            return Some("another mega-block is rising".into());
        }
        None
    }

    // ── building one ──────────────────────────────────────────────────────

    /// [megablocks] Corp `ci` breaks ground on its mega-block: the site
    /// (`megablock_site`) bought, what stands on it pulled down (residents
    /// move out with a memory, businesses look for a new place), and a long
    /// build of four stages. `force` skips the gates (not the site rules).
    pub fn start_megablock(&mut self, ci: usize, force: bool) -> Result<BuildingId, String> {
        if ci >= self.corps.corps.len() {
            return Err("no such corp".into());
        }
        if !force {
            if let Some(why) = self.megablock_refusal(ci) {
                return Err(why);
            }
        } else if self.corps.corps[ci].megablock.is_some_and(|b| self.buildings.contains_key(b)) {
            return Err(format!("{} has its mega-block already", self.corps.corps[ci].brand.name));
        }
        let owner = self.corps.corps[ci].owner;
        let site = self.megablock_primaries(ci).into_iter().take(120).find_map(|p| self.megablock_site(p, owner));
        let Some(plots) = site else {
            return Err("no block of plots round its holdings could be put together (needs 4,500 m², compact, homes, shops or leisure land, low buildings)".into());
        };
        Ok(self.break_megablock_ground(ci, &plots))
    }

    fn break_megablock_ground(&mut self, ci: usize, plots: &[LotId]) -> BuildingId {
        let primary = plots[0];
        let (owner, corp_name) = (self.corps.corps[ci].owner, self.corps.corps[ci].brand.name.clone());
        let short = corp_name.split_whitespace().next().unwrap_or(&corp_name).to_string();
        let area: f32 = plots.iter().filter_map(|l| self.lots.get(*l)).map(|l| l.area()).sum();
        let res_m = crate::towers::FLOOR_M[LandUse::Residential.index()];
        let limit = plots.iter().filter_map(|l| self.height_limit_at(*l)).fold(f32::MAX, f32::min);
        let max_tower = if limit < f32::MAX { ((limit / res_m) as i32 - PODIUM_FLOORS as i32).clamp(10, 250) as u8 } else { TOWER_MAX_FLOORS };
        let seed = self.fresh_seed();
        let day = self.clock.day() as u32;
        let plan = MegaBlock::plan(seed, owner, area, max_tower, day);
        let floors = plan.total_floors();
        let name = match crate::hash::mix(seed, 0x4A3E) % 4 {
            0 => format!("{short} Arcology"),
            1 => format!("{short} Habitat"),
            2 => format!("{short} City Block"),
            _ => format!("{short} Megablock"),
        };
        let value = self.lot_value(primary);
        let price = self.plots_price(plots);
        let memory = format!("{corp_name} bought up our block for {name}. We had to go.");
        let bid = match self.lots[primary].building {
            Some(b) => {
                self.pack_business(b, &name);
                self.settle_sale_to(b, Some(owner));
                self.buildings[b].land_use = LandUse::Residential;
                self.redevelop_with(b, Tier::Mid, floors, value, memory.clone(), true);
                self.tag_demolition(b, crate::buildings::DemolitionReason::Landmark);
                b
            }
            None => {
                let mut b = Building::new(primary, LandUse::Residential, floors, Tier::Mid, seed, name.clone(), self.clock.tick, value);
                b.construction = Some(Construction::new(1.0));
                let id = self.buildings.insert(b);
                self.lots[primary].building = Some(id);
                self.events.push(SimEvent::BuildingBuilt(id));
                self.order_materials(id);
                id
            }
        };
        if self.buildings[bid].business.is_some() {
            self.set_business(bid, None);
        }
        {
            let b = &mut self.buildings[bid];
            b.name = name.clone();
            b.look = None;
            b.plot.form = Some(Form::MegaBlock);
            b.plot.megablock = Some(plan);
            b.high_land_days = 0;
            if let Some(c) = b.construction.as_mut() {
                c.days = MEGABLOCK_BUILD_DAYS;
            }
            b.recompute_capacity();
            b.update_rent(value);
        }
        self.take_plots_for(bid, &plots[1..], &memory, crate::buildings::DemolitionReason::Landmark);
        self.give_to(bid, owner, price);
        self.corps.corps[ci].megablock = Some(bid);
        // [ledger] The deposit on the land and the works, to the sellers and
        // builders out of town (the bank lends the rest).
        let cash = self.corps.corps[ci].cash.max(0.0);
        let deposit = ((price + 2.0e7) * crate::corp::DEPOSIT_SHARE * 4.0).min(cash * 0.8);
        self.transfer64(Account::Owner(owner), Account::Outside, deposit, Kind::PropertySale);
        self.note_owner(owner, format!("broke ground on {name}, its mega-block, over {} plots", plots.len()));
        self.megablocks.started_total += 1;
        self.megablocks.last_refusal = None;
        let tick = self.clock.tick;
        let pos = self.footprint(bid).map(|l| l.center).unwrap_or_else(|| self.lots[primary].center);
        let headline = format!("{corp_name} breaks ground on {name}: {} towers, {} homes, {} shops and sky gardens over {} plots", plan.towers, plan.homes, plan.shops, plots.len());
        self.skyline.push_news(tick, headline, pos, Some(bid));
        self.events.push(SimEvent::BuildingChanged(bid));
        self.events.push(SimEvent::LotsChanged);
        bid
    }

    /// [megablocks] A mega-block's works are done (`finish_construction`,
    /// after its units are planned): it opens, and its corp opens its own
    /// market, gym, café, noodle bar, bar and back offices in it; the rest
    /// is let over the days.
    pub(super) fn megablock_opened(&mut self, bid: BuildingId) {
        let Some(m) = self.buildings.get(bid).and_then(|b| b.plot.megablock) else { return };
        if m.opened_day.is_some() {
            return;
        }
        let day = self.clock.day() as u32;
        if let Some(x) = self.buildings[bid].plot.megablock.as_mut() {
            x.opened_day = Some(day);
        }
        self.megablocks.opened_total += 1;
        let corp = self.estate.owners.get(m.corp).map_or_else(|| "The corp".to_string(), |o| o.name.clone());
        let short = corp.split_whitespace().next().unwrap_or(&corp).to_string();
        use crate::buildings::UnitRole;
        use BusinessKind as K;
        let opening: [(K, &str); 7] = [(K::Grocery, "Market"), (K::Gym, "Fitness"), (K::Cafe, "Sky Café"), (K::NoodleBar, "Noodle Deck"), (K::Bar, "Lounge"), (K::Bakery, "Bakehouse"), (K::GadgetShop, "Tech")];
        let mut k = 0;
        let shops: Vec<usize> = self.buildings[bid].units.iter().enumerate().filter(|(_, u)| u.shopfront && u.role == UnitRole::Shop).map(|(i, _)| i).collect();
        for i in shops {
            if k >= opening.len() {
                break;
            }
            let (kind, label) = opening[k];
            k += 1;
            let jobs = self.buildings[bid].units[i].jobs;
            let mut z = Business::new(kind, jobs);
            z.origin = Some(format!("Opened by {corp} with {}.", self.buildings[bid].name));
            let _ = self.open_tenant(bid, i, z, Some(format!("{short} {label}")));
        }
        let offices: Vec<usize> = self.buildings[bid].units.iter().enumerate().filter(|(_, u)| !u.shopfront).map(|(i, _)| i).take(2).collect();
        for (n, i) in offices.into_iter().enumerate() {
            let jobs = self.buildings[bid].units[i].jobs;
            let mut z = Business::new(K::Office, jobs);
            z.origin = Some(format!("{corp}'s offices in its own block."));
            let _ = self.open_tenant(bid, i, z, Some(if n == 0 { format!("{short} Operations") } else { format!("{short} Labs") }));
        }
        let name = self.buildings[bid].name.clone();
        let tick = self.clock.tick;
        let pos = self.footprint(bid).map(|l| l.center).unwrap_or_default();
        self.skyline.push_news(tick, format!("{name} opens: {} homes, {} shops, offices and {} sky gardens under one roof", m.homes, m.shops, m.gardens()), pos, Some(bid));
        self.note_owner(m.corp, format!("opened {name}"));
        self.events.push(SimEvent::BuildingChanged(bid));
    }

    // ── the day ───────────────────────────────────────────────────────────

    /// [megablocks] Once a day after the corps: each block's tallies roll,
    /// residents working outside take the free jobs inside, and maybe one
    /// corp breaks ground.
    pub(super) fn daily_megablocks(&mut self) {
        let blocks = self.megablock_list();
        for &bid in &blocks {
            if let Some(m) = self.buildings[bid].plot.megablock.as_mut() {
                m.roll_day();
            }
            if self.megablock_of(bid) == Some(bid) {
                self.move_jobs_inside(bid);
            }
        }
        // A corp whose block was pulled down may build another.
        for c in &mut self.corps.corps {
            if c.megablock.is_some_and(|b| !blocks.contains(&b)) {
                c.megablock = None;
            }
        }
        let day = self.clock.day();
        for ci in 0..self.corps.corps.len() {
            if self.megablock_refusal(ci).is_some() {
                continue;
            }
            use slotmap::Key;
            let h = self.corps.corps[ci].owner.data().as_ffi();
            if hash01(h, day, 0x3E6A_B10C) >= MEGABLOCK_DAILY_CHANCE {
                continue;
            }
            match self.start_megablock(ci, false) {
                Ok(_) => break,
                Err(why) => self.megablocks.last_refusal = Some(why),
            }
        }
    }

    /// [megablocks] Up to [`JOB_MOVES_PER_DAY`] residents of `bid` who work
    /// outside (and are not at work now) take a free job inside.
    fn move_jobs_inside(&mut self, bid: BuildingId) {
        let mut moved = 0;
        let residents = self.buildings[bid].residents.clone();
        for cid in residents {
            if moved >= JOB_MOVES_PER_DAY {
                break;
            }
            let Some(c) = self.citizens.get(cid) else { continue };
            let Some(old) = c.work else { continue };
            if self.megablock_of(old) == Some(bid) || c.inside == Some(old) || self.buildings.get(old).is_some_and(|b| b.construction.is_some()) {
                continue;
            }
            let Some(job) = self.megablock_job_in(bid) else { break };
            if let Some(b) = self.buildings.get_mut(old) {
                b.workers.retain(|w| *w != cid);
            }
            let old_name = self.buildings.get(old).map_or_else(String::new, |b| b.name.clone());
            self.assign_job(cid, job);
            let tick = self.clock.tick;
            let name = self.buildings[job].name.clone();
            let c = &mut self.citizens[cid];
            c.remember(tick, format!("Left {old_name} for {name}, downstairs. No more commute."));
            c.thought = "Work's a lift ride away now.".to_string();
            moved += 1;
            if let Some(m) = self.buildings[bid].plot.megablock.as_mut() {
                m.moved_in_jobs_total += 1;
            }
            self.megablocks.jobs_moved_inside_total += 1;
        }
    }

    /// [megablocks] A free job in the block `bid`: the open tenant with the
    /// most posts to fill (ties by unit order).
    fn megablock_job_in(&self, bid: BuildingId) -> Option<BuildingId> {
        self.tenants_of(bid)
            .into_iter()
            .filter_map(|t| self.buildings.get(t).filter(|b| b.operating() && b.business.as_ref().is_some_and(|z| !z.closed)).map(|b| (b.free_jobs(), t)))
            .filter(|(n, _)| *n > 0)
            .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
            .map(|x| x.1)
    }

    /// [megablocks] A free job inside the block someone lives in, for the
    /// job search: downstairs before across town.
    pub(super) fn megablock_job_for(&self, cid: CitizenId) -> Option<BuildingId> {
        if self.megablocks.opened_total == 0 {
            return None;
        }
        let home = self.citizens.get(cid)?.home?;
        let block = self.megablock_of(home)?;
        self.megablock_job_in(block)
    }

    // ── trips ─────────────────────────────────────────────────────────────

    /// [megablocks] A trip from `origin` to `target` in the same open
    /// mega-block: by lift and corridor, never on the streets (counted);
    /// true if it was one (the citizen is on the way). A trip from inside to
    /// the streets is counted as one out.
    pub(super) fn trip_inside_megablock(&mut self, id: CitizenId, origin: Option<BuildingId>, target: BuildingId, purpose: Purpose) -> bool {
        if self.megablocks.opened_total == 0 {
            return false;
        }
        let Some(from) = origin.and_then(|o| self.megablock_of(o)) else { return false };
        if self.megablock_of(target) != Some(from) {
            if let Some(m) = self.buildings[from].plot.megablock.as_mut() {
                m.out_today += 1;
            }
            return false;
        }
        let Some(at) = self.building_entrance(target) else { return false };
        self.leave_building(id);
        let c = &mut self.citizens[id];
        c.pos = at;
        c.prev_pos = at;
        c.mode = TravelMode::Walk;
        c.activity = Activity::Traveling { to: Some(target), purpose, path: vec![at], idx: 0 };
        if let Some(m) = self.buildings[from].plot.megablock.as_mut() {
            m.inside_today += 1;
            m.inside_total += 1;
        }
        self.megablocks.inside_trips_total += 1;
        true
    }

    /// [megablocks] Someone at home in a mega-block who fancies a park goes
    /// up to one of its sky gardens instead (by [`GARDEN_CHANCE`], a hash):
    /// a lift ride, an hour among the planters. True if they did.
    pub(super) fn sky_garden_visit(&mut self, id: CitizenId) -> bool {
        if self.megablocks.opened_total == 0 {
            return false;
        }
        let tick = self.clock.tick;
        let Some(c) = self.citizens.get(id) else { return false };
        let Some(home) = c.home.filter(|h| c.inside == Some(*h)) else { return false };
        let Some(block) = self.megablock_of(home) else { return false };
        if hash01(c.seed, tick, 0x5C1_6A2D) >= GARDEN_CHANCE {
            return false;
        }
        let Some(m) = self.buildings[block].plot.megablock else { return false };
        let tower = (crate::hash::mix(c.seed, 0x70E2) % m.towers.max(1) as u64) as u8;
        let levels = (m.floors_of(tower) / GARDEN_EVERY).max(1);
        let level = m.podium_floors as u32 + GARDEN_EVERY as u32 * (1 + (crate::hash::mix(c.seed, tick) % levels as u64) as u32);
        let name = self.buildings[block].name.clone();
        let c = &mut self.citizens[id];
        c.mood = (c.mood + 0.06).min(1.0);
        c.thought = format!("Sky garden, floor {level}. The air's better up here.");
        c.remember(tick, format!("An hour in the sky garden on floor {level} of {name}."));
        c.next_decision_tick = tick + 45 + crate::hash::mix(c.seed, tick ^ 0x9A) % 60;
        if let Some(x) = self.buildings[block].plot.megablock.as_mut() {
            x.gardens_today += 1;
        }
        self.megablocks.garden_visits_total += 1;
        true
    }

    // ── round it ──────────────────────────────────────────────────────────

    /// [megablocks] Land value: an address by a mega-block is worth more,
    /// out to [`LIFT_RADIUS`]; right under it, its shade takes some off.
    pub(super) fn splat_megablocks(&self, b: &mut crate::land_value::LandValueBuilder) {
        for (id, bld) in self.buildings.iter() {
            if bld.plot.megablock.is_none() || !bld.operating() {
                continue;
            }
            let Some(c) = self.footprint(id).map(|l| l.center) else { continue };
            b.splat(c, LIFT_RADIUS, LIFT_WEIGHT);
            b.splat_nuisance(c, SHADE_RADIUS, SHADE_WEIGHT);
        }
    }

    /// [megablocks] For probes and screenshots: corp `ci` (the first that
    /// can) starts its mega-block now, and with `finish` it is built and
    /// opened at once (the sites cleared, the units planned). Its id.
    #[doc(hidden)]
    pub fn force_megablock(&mut self, corp: Option<OwnerId>, progress: Option<f32>) -> Result<BuildingId, String> {
        let order: Vec<usize> = match corp {
            Some(o) => vec![self.corps.corps.iter().position(|c| c.owner == o).ok_or("no such corp")?],
            None => (0..self.corps.corps.len()).collect(),
        };
        if order.is_empty() {
            return Err("no mega-corps yet".into());
        }
        let mut why = String::new();
        for ci in order {
            let bid = match self.corps.corps[ci].megablock.filter(|b| self.buildings.contains_key(*b)) {
                Some(b) => b,
                None => match self.start_megablock(ci, true) {
                    Ok(b) => b,
                    Err(e) => {
                        why = e;
                        continue;
                    }
                },
            };
            // The demolitions on the bought plots, cleared.
            let pending = self.buildings[bid].plot.pending_lots.clone();
            for l in pending {
                if let Some(other) = self.lots.get(l).and_then(|x| x.building).filter(|o| *o != bid) {
                    if self.buildings.get(other).is_some_and(|o| o.demolition.is_some()) {
                        self.finish_demolition(other);
                    }
                }
                self.claim_reserved(l);
            }
            match progress {
                Some(p) if p < 1.0 => {
                    if self.buildings[bid].demolition.is_some() {
                        self.finish_demolition(bid);
                    }
                    if let Some(c) = self.buildings[bid].construction.as_mut() {
                        c.progress = p.clamp(0.0, 0.999);
                        c.deliveries = c.deliveries.max(c.stage() + 1);
                    }
                    self.events.push(SimEvent::BuildingChanged(bid));
                }
                _ => {
                    if self.buildings[bid].construction.is_some() {
                        self.finish_construction(bid);
                    }
                }
            }
            return Ok(bid);
        }
        Err(why)
    }
}

/// Owner kinds a mega-block is built by (the API's check).
pub fn builds_megablocks(k: &OwnerKind) -> bool {
    matches!(k, OwnerKind::MegaCorp { .. })
}
