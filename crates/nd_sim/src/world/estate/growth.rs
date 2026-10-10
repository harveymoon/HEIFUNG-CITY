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
        // [pt-growth] A busy town rebuilds several a day (the old cheap-on-
        // dear-land rule rebuilt whole districts at once on the saturated
        // land value; this is where that growth comes from now).
        let max = (2 + self.buildings.len() / 200).min(6);
        let tick = self.clock.tick;
        let mut cands: Vec<(u8, BuildingId)> = Vec::new();
        for (id, b) in self.buildings.iter() {
            // [pt-growth] A developer's own building (a block it assembled
            // or rebuilt before) may be rebuilt taller again as the street
            // gets busier: density feeds on itself.
            let developers = b.plot.owner.and_then(|o| self.estate.owners.get(o)).is_some_and(|o| o.kind == OwnerKind::Developer);
            let free = b.plot.owner.is_none() || developers;
            let form_ok = b.plot.form.is_none() || (developers && b.plot.form == Some(Form::Merged));
            if !matches!(b.land_use, LandUse::Residential | LandUse::Commercial | LandUse::Entertainment) || !form_ok || !free {
                continue;
            }
            if tick.saturating_sub(b.built_tick) < 3 * 1440 || !self.buyable(id) {
                continue;
            }
            // [pt-growth] Only uses people want more of: a street of homes
            // nobody needs no longer crowds the shops the town is short of
            // out of the day's few tries.
            if self.economy.demand_for(b.land_use) < 0.2 {
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
            // [pt-growth] Small plots are bought up together for one bigger
            // building; a big plot (or one with no small neighbours to buy)
            // is rebuilt on its own.
            if self.assemble_plots(bid, &mut rng) || self.buy_out_and_rebuild(bid, &mut rng) {
                done += 1;
            }
        }
    }

    /// [pt-growth] Lot assembly: `bid` stands on a small plot where the
    /// land density asks for at least `ASSEMBLY_MIN_FLOORS`; a developer
    /// buys it with two to four small, low neighbours of the same use
    /// (vacant ones too) that make one compact block, pulls them all down
    /// and puts up one building across them, as tall as the land density
    /// and the neighbours allow. Its residents and its neighbours' move
    /// out (and remember why), businesses pack up to move. True if it went
    /// ahead.
    pub fn assemble_plots(&mut self, bid: BuildingId, rng: &mut SimRng) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        let (lu, lot, old_floors) = (b.land_use, b.lot, b.floors);
        if !matches!(lu, LandUse::Residential | LandUse::Commercial) || b.plot.merged() || old_floors > ASSEMBLY_MAX_OLD_FLOORS || !self.buyable(bid) {
            return false;
        }
        if self.lots.get(lot).is_none_or(|l| l.area() >= ASSEMBLY_SMALL_LOT || l.landlocked) {
            return false;
        }
        let cap = self.height_cap(lot);
        let want = self.pressure_floors(lot).min(cap);
        if want < ASSEMBLY_MIN_FLOORS.max(old_floors + 2) {
            return false;
        }
        let owner = b.plot.owner;
        let tick = self.clock.tick;
        // Grow the block a plot at a time, cheapest touching plot first.
        let mut ours = vec![lot];
        while ours.len() < ASSEMBLY_MAX_LOTS {
            let mut added = false;
            for (_, l) in self.plots_next_to(&ours, lu, ASSEMBLY_MAX_OLD_FLOORS, owner) {
                let Some(x) = self.lots.get(l) else { continue };
                if x.area() >= ASSEMBLY_SMALL_LOT * 1.5 || self.estate.reserved.contains_key(&l) {
                    continue;
                }
                if let Some(o) = x.building {
                    let Some(ob) = self.buildings.get(o) else { continue };
                    if ob.plot.form.is_some() || tick.saturating_sub(ob.built_tick) < 3 * 1440 {
                        continue;
                    }
                }
                let others: Vec<&Lot> = ours.iter().skip(1).chain(std::iter::once(&l)).filter_map(|i| self.lots.get(*i)).collect();
                if merged_lot_within(&self.lots[lot], &others, ASSEMBLY_HULL_SLACK).is_none() {
                    continue;
                }
                ours.push(l);
                added = true;
                break;
            }
            if !added {
                break;
            }
        }
        if ours.len() < ASSEMBLY_MIN_LOTS {
            return false;
        }
        let picked: Vec<LotId> = ours[1..].to_vec();
        let value = self.lot_value(lot);
        let mut tier = Tier::for_land_value(value);
        if lu == LandUse::Residential {
            tier = self.residential_tier(tier);
            if self.short_of_cheap_homes() {
                tier = tier.min(self.buildings[bid].tier);
            }
        }
        if let Some(l) = self.buildings[bid].look {
            tier = l.tier(tier);
        }
        let floors = self.pick_floors(lot, lu, tier).max(want.min(ASSEMBLY_MIN_FLOORS + 2)).clamp(ASSEMBLY_MIN_FLOORS, cap.max(ASSEMBLY_MIN_FLOORS));
        // The sum must pay: every plot at its price against the new block
        // (its land and the homes or jobs across all the plots).
        let price = self.sale_price(bid) + self.plots_price(&picked);
        let area: f32 = ours.iter().filter_map(|l| self.lots.get(*l)).map(|l| l.area()).sum();
        let mut n = self.buildings[bid].clone();
        n.tier = tier;
        n.floors = floors;
        n.construction = None;
        n.demolition = None;
        n.business = None;
        n.plot.form = Some(Form::Merged);
        n.plot.extra_lots = picked.clone();
        n.recompute_capacity();
        let per_unit = match tier {
            Tier::Low => 15_000.0,
            Tier::Mid => 40_000.0,
            Tier::High => 120_000.0,
        };
        let worth = area as f64 * (25.0 + 400.0 * value.max(0.0) as f64) + (n.homes as f64 + n.jobs as f64) * per_unit;
        if worth < price * 1.2 {
            return false;
        }
        let dev = self.developer(rng);
        let dev_name = self.estate.owners.get(dev).map(|o| o.name.clone()).unwrap_or_default();
        let (name, had_business) = (self.buildings[bid].name.clone(), self.buildings[bid].business.is_some());
        let memory = format!("{dev_name} bought up {name} and the houses next to it for a new block of {floors} floors. We had to go.");
        if had_business {
            self.pack_business(bid, &format!("{dev_name}'s new block"));
        }
        self.settle_sale(bid);
        self.buildings[bid].plot.form = Some(Form::Merged);
        self.take_plots_for(bid, &picked, &memory, crate::buildings::DemolitionReason::Assembly);
        self.redevelop_with(bid, tier, floors, value, memory, false);
        self.tag_demolition(bid, crate::buildings::DemolitionReason::Assembly);
        if had_business || lu == LandUse::Commercial {
            let jobs = self.buildings[bid].planned_capacity().1;
            let kind = self.pick_business_kind(lu, jobs);
            self.buildings[bid].business = kind.map(|k| Business::new(k, jobs));
        }
        self.give_to(bid, dev, price);
        let s = &mut self.estate.stats;
        s.assemblies_total += 1;
        s.assembled_lots_total += ours.len() as u32;
        s.buyouts_total += 1;
        s.buyouts_today += 1;
        let (tick, pos) = (self.clock.tick, self.lots.get(lot).map(|l| l.center).unwrap_or_default());
        self.skyline.push_news(tick, format!("{dev_name} buys {} plots round {name} for a {floors}-floor block", ours.len()), pos, Some(bid));
        true
    }

    /// [pt-growth] People in town (on the street, or turned away at the
    /// edge) who could not find a home they can afford.
    pub fn short_of_cheap_homes(&self) -> bool {
        self.economy.housing_demand_by_tier[Tier::Low.index()] > 2.0 || self.homeless_people() > 20
    }

    /// [pt-growth] Owners can lose their buildings: a block of homes left
    /// mostly empty for `FORECLOSE_DAYS` (the rents too dear for the street,
    /// the tenants gone elsewhere) while people in town wait for a home
    /// they can afford is sold cheap to a developer, who cuts the rent: one
    /// tier down, refitted (the rent follows), or rebuilt bigger where the
    /// land density asks for it. A few a day.
    pub(in crate::world) fn foreclose_empty(&mut self) {
        let mut rng = self.erng(erng_salt::FORECLOSE);
        let waiting = self.economy.waiting_immigrants > 0 || self.homeless_people() > 0;
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        let mut sold = 0;
        for bid in ids {
            let b = &self.buildings[bid];
            if b.land_use != LandUse::Residential || b.homes < 4 || !b.operating() || !b.is_grown() || b.plot.form.is_some_and(|f| f.owner_occupied()) {
                continue;
            }
            let full = b.households.len() as f32 / b.homes.max(1) as f32;
            let empty = full <= FORECLOSE_OCCUPANCY && b.tier > Tier::Low;
            let days = if empty { b.empty_days.saturating_add(1) } else { 0 };
            self.buildings[bid].empty_days = days;
            if days < FORECLOSE_DAYS || !waiting || sold >= 2 || !self.buyable(bid) {
                continue;
            }
            let developers = self.buildings[bid].plot.owner.and_then(|o| self.estate.owners.get(o)).is_some_and(|o| o.kind == OwnerKind::Developer);
            if developers && !rng.random_bool(0.3) {
                // A developer that already holds it tries the market a while.
                continue;
            }
            let price = self.sale_price(bid) * FORECLOSE_DISCOUNT;
            let dev = self.developer(&mut rng);
            let name = self.buildings[bid].name.clone();
            let dev_name = self.estate.owners.get(dev).map(|o| o.name.clone()).unwrap_or_default();
            self.settle_sale(bid);
            self.give_to(bid, dev, price);
            let tier = self.buildings[bid].tier;
            let b = &mut self.buildings[bid];
            b.empty_days = 0;
            b.tier = tier.down();
            b.condition = b.condition.max(0.8);
            let seed = self.fresh_seed();
            let value = self.lot_value(self.buildings[bid].lot);
            let b = &mut self.buildings[bid];
            b.seed = seed;
            b.recompute_capacity();
            b.update_rent(value);
            self.events.push(SimEvent::BuildingChanged(bid));
            self.estate.stats.foreclosures_total += 1;
            let (tick, pos) = (self.clock.tick, self.building_lot(bid).map(|l| l.center).unwrap_or_default());
            self.skyline.push_news(tick, format!("{name} stood half empty: its owner lost it to {dev_name}, who is cutting the rents"), pos, Some(bid));
            sold += 1;
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
            // [pt-growth] While people cannot afford a home, cheap homes
            // are rebuilt cheap (taller, more of them), not dearer.
            if self.short_of_cheap_homes() {
                tier = tier.min(self.buildings[bid].tier);
            }
        }
        if let Some(l) = self.buildings[bid].look {
            tier = l.tier(tier);
        }
        // [pt-growth] Built towards the land density a step at a time: up
        // to three times as tall (and two floors) as what stood, never past
        // what it and the neighbours allow, so a street of walk-ups becomes
        // mid-rise and only later (rebuilt again as it gets busier) towers.
        let cap = self.height_cap(lot);
        let want = self.pressure_floors(lot).min(cap).min(old_floors.saturating_mul(3).saturating_add(2));
        let floors = self.pick_floors(lot, lu, tier).max(old_floors + 1).max(want).min(cap.max(old_floors + 1)).min(old_floors.saturating_mul(3).saturating_add(2).max(old_floors + 1));
        // A better use, or no sale: what the new building is worth (land
        // and structure, no goodwill yet) must clear the price by a fifth.
        // A thriving business's goodwill keeps it off the market.
        if self.value_of_new(bid, tier, floors) < price * 1.2 {
            return false;
        }
        if self.buildings[bid].plot.owner.and_then(|o| self.estate.owners.get(o)).is_some_and(|o| o.kind == OwnerKind::Developer) {
            self.estate.stats.redensified_total += 1;
        }
        let dev_name = self.estate.owners.get(dev).map(|o| o.name.clone()).unwrap_or_default();
        let memory = format!("{name} was sold to {dev_name} for ${:.0}k and is coming down for something bigger. We had to go.", price / 1000.0);
        // [relocate] The business looks for a new place.
        if had_business {
            self.pack_business(bid, &format!("{dev_name}'s rebuild"));
        }
        self.settle_sale(bid);
        self.redevelop_with(bid, tier, floors, value, memory, false);
        self.tag_demolition(bid, crate::buildings::DemolitionReason::Redevelopment);
        if had_business {
            // The old business closes with the sale; the new building
            // opens as what the market wants now.
            let jobs = self.buildings[bid].planned_capacity().1;
            let kind = self.pick_business_kind(lu, jobs);
            self.buildings[bid].business = kind.map(|k| Business::new(k, jobs));
            self.fit_business_name(bid);
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
        self.tag_demolition(bid, crate::buildings::DemolitionReason::Expansion);
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
                self.tag_demolition(b, crate::buildings::DemolitionReason::Estate);
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
        self.take_plots_for(bid, &rest, memory, crate::buildings::DemolitionReason::Estate);
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
