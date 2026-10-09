//! [land] The market growing the town: densification, firms expanding,
//! homes bought outright and built big.

use super::*;

impl World {
    /// Where the city presses for more than stands, developers buy the low
    /// buildings out, pull them down and build bigger: the most pressed
    /// first, a few a day. Residents move, a business closes (the new
    /// building opens as what the market wants).
    pub(in crate::world) fn densify(&mut self) {
        let mut rng = self.erng(erng_salt::DENSIFY);
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
        self.settle_sale(bid);
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
    pub(in crate::world) fn expand_firms(&mut self) {
        let mut rng = self.erng(erng_salt::EXPAND_FIRMS);
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
            if !self.lot_free_for(lid, SiteRule::STREET) {
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
        let price: f64 = self.plots_price(&picked);
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

    /// A well-off family buys two plots side by side in a quiet, good
    /// suburb and builds one big house with a pool: at most one a day, and
    /// one for every six well-off households.
    pub(in crate::world) fn build_mcmansions(&mut self) {
        let mut rng = self.erng(erng_salt::MCMANSIONS);
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
        if self.acquire(&pair, Form::McMansion, owner, Some(hid), 2, &format!("The {family} family bought our place to build a big house. We had to go.")).is_some() {
            self.estate.stats.mcmansions_total += 1;
        }
    }

    /// Two touching residential plots in a low-density, well-valued block,
    /// empty or holding low homes the market may buy, that make one compact
    /// plot. The best valued first.
    pub(super) fn mcmansion_site(&self) -> Option<Vec<LotId>> {
        let mut cands: Vec<(f32, LotId)> = Vec::new();
        for (lid, l) in self.lots.iter() {
            if l.land_use != LandUse::Residential || !self.site_clear(lid, SiteRule::ANY_NOT_HISTORIC) {
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
    pub(super) fn acquire(&mut self, plots: &[LotId], form: Form, owner: OwnerId, buyer: Option<HouseholdId>, floors: u8, memory: &str) -> Option<BuildingId> {
        let primary = plots.iter().copied().find(|l| self.lots.get(*l).is_some_and(|x| x.building.is_some() && !x.landlocked)).or_else(|| plots.iter().copied().find(|l| self.lots.get(*l).is_some_and(|x| !x.landlocked)))?;
        let price: f64 = self.plots_price(&plots);
        let value = self.lot_value(primary);
        let bid = match self.lots[primary].building {
            Some(b) => {
                // [relocate] A business on the plot looks for a new place.
                let why = match form {
                    Form::Mansion => "a billionaire's estate".to_string(),
                    _ => "a family's new house".to_string(),
                };
                self.pack_business(b, &why);
                self.settle_sale(b);
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
                    let mut rng = self.erng(erng_salt::ACQUIRE_NAME);
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
    pub(in crate::world) fn estate_finished(&mut self, bid: BuildingId) {
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
}
