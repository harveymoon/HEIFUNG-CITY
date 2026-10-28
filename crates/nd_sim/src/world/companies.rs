//! [companies] Local companies on the `World` (`crate::companies`): the
//! builders that took the out-of-town developers' place, old saves'
//! developers selling up to them, firms growing, renaming and merging,
//! shops and yards growing into chains, fleet liveries, a corp's
//! specialty, and the company directory's rows.
//!
//! Costs: a pass over the owners and the buildings a day; the directory
//! rows (`company_summaries`) one pass over owners, buildings, cabs, trucks
//! and cars when asked. Draws come from the estate's hash-seeded
//! generators and hashes, never the world's own stream.

use super::*;
use crate::companies::*;
use crate::estate::{branches, expansion_form, OwnerId, OwnerKind, MAX_MERGED_LOTS};
use crate::vehicle::Livery;
use slotmap::Key;
use std::collections::BTreeMap;

/// [companies] One owner as the company directory and "Biggest companies"
/// show it.
#[derive(Clone, Debug, PartialEq)]
pub struct CompanySummary {
    pub owner: OwnerId,
    pub name: String,
    /// What it is, in words: "builder and landlord", "mega-corp (grocery)".
    pub what: String,
    /// Its kind's short key: "builder", "firm", "founder", "property",
    /// "corp", "university", "billionaire", "family", "developer".
    pub kind_key: &'static str,
    /// The resident behind it, by name, if any.
    pub founder: Option<String>,
    pub properties: usize,
    /// What its buildings would sell for, summed (`World::sale_price`).
    pub value: f64,
    /// Rent collected yesterday, and its businesses' takings yesterday.
    pub rent: f32,
    pub takings: f32,
    pub staff: usize,
    /// Vehicles it runs now: cabs, tow and garbage trucks, lorries and vans
    /// on the road.
    pub vehicles: usize,
    pub money: f64,
    pub since_day: u32,
    /// Its newest history line ("day 63: renamed from ..."), and all of them.
    pub history: Vec<crate::companies::OwnerNote>,
    /// Its fleet colour, if it runs vehicles.
    pub livery: Option<u8>,
    /// A building to select and fly to: its HQ, its biggest, its first.
    pub flagship: Option<BuildingId>,
    /// Size for the ranking: the value of what it owns and a year of its
    /// rent and takings.
    pub size: f64,
}

/// The owner kind's short key and its words.
pub fn owner_kind_words(w: &World, oid: OwnerId, k: &OwnerKind) -> (&'static str, String) {
    match k {
        OwnerKind::Firm { kind } => ("firm", format!("{} firm", kind.label())),
        OwnerKind::Developer => ("developer", "out-of-town developer (selling up to local firms)".into()),
        OwnerKind::Household { .. } => ("family", "family (owner-occupied)".into()),
        OwnerKind::Elite { .. } => ("billionaire", "billionaire".into()),
        OwnerKind::Person { .. } => ("founder", "resident who founded their business".into()),
        OwnerKind::MegaCorp { kind } => {
            let spec = w.corps.corp_of_owner(oid).map_or(Specialty::of_origin(*kind), |c| c.specialty());
            ("corp", format!("mega-corp from {} ({})", kind.label(), spec.label()))
        }
        OwnerKind::University { .. } => ("university", "university".into()),
        OwnerKind::Company { .. } => ("property", "resident's property company".into()),
        OwnerKind::Builder { .. } => ("builder", "local builder and landlord".into()),
    }
}

impl World {
    // ── history ────────────────────────────────────────────────────────────

    /// Add a line to an owner's history (the oldest go past `HISTORY_MAX`).
    pub(in crate::world) fn note_owner(&mut self, oid: OwnerId, text: String) {
        let day = self.clock.day() as u32;
        if let Some(o) = self.estate.owners.get_mut(oid) {
            o.history.push(OwnerNote { day, text });
            if o.history.len() > HISTORY_MAX {
                let n = o.history.len() - HISTORY_MAX;
                o.history.drain(..n);
            }
        }
    }

    /// Where an owner's holdings are, on average (`None` for no holdings).
    pub fn owner_centre(&self, oid: OwnerId) -> Option<Vec2> {
        let o = self.estate.owners.get(oid)?;
        let pts: Vec<Vec2> = o.properties.iter().filter_map(|p| self.building_lot(*p)).map(|l| l.center).collect();
        (!pts.is_empty()).then(|| pts.iter().fold(Vec2::ZERO, |a, p| a + *p) / pts.len() as f32)
    }

    // ── builders ───────────────────────────────────────────────────────────

    /// How many builder firms the town carries.
    pub fn builder_room(&self) -> usize {
        (1 + self.citizens.len() / BUILDER_PEOPLE_EACH).min(BUILDERS_MAX)
    }

    /// The local builder that buys the plot `lot`: the nearest by its
    /// roots (its oldest building), a bigger firm counting as further off
    /// (`BUILDER_SIZE_REACH` buildings double the distance) so one firm
    /// does not buy the whole town, while that comes within
    /// `BUILDER_REACH` (three times in four, so a rival may still win it);
    /// else a new one founded by a resident who lives near, while the town
    /// has room for one; else the nearest. Tower developers build towers
    /// only.
    pub(in crate::world) fn builder_for(&mut self, lot: LotId, rng: &mut SimRng) -> OwnerId {
        let at = self.lots.get(lot).map_or(Vec2::ZERO, |l| l.center);
        let towers: Vec<OwnerId> = self.skyline.developers.iter().filter_map(|d| d.owner).collect();
        let mut near: Vec<(f32, OwnerId)> = self
            .estate
            .owners
            .iter()
            .filter(|(id, o)| matches!(o.kind, OwnerKind::Builder { .. }) && !o.properties.is_empty() && !towers.contains(id))
            .filter_map(|(id, o)| {
                let roots = o.properties.iter().find_map(|p| self.building_lot(*p))?.center;
                Some((roots.distance(at) * (1.0 + o.properties.len() as f32 / BUILDER_SIZE_REACH), id))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let room = near.len() < self.builder_room();
        if let Some(&(d, o)) = near.first() {
            if d <= BUILDER_REACH && (!room || rng.random_bool(0.75)) {
                return o;
            }
            if !room {
                return o;
            }
        }
        self.found_builder(at)
    }

    /// A new builder and landlord firm, founded by the resident with the
    /// most to put in who lives near `at` (a kilometre away counts as 200
    /// less), named after them. A town with nobody in it yet (tests) gets
    /// one named from a hash.
    pub(in crate::world) fn found_builder(&mut self, at: Vec2) -> OwnerId {
        let founder = self.found_builder_founder(at);
        let h = crate::hash::mix(self.config.seed ^ 0xB1_1D, (at.x.to_bits() as u64) << 32 ^ at.y.to_bits() as u64 ^ self.clock.tick);
        let surname = match founder.and_then(|f| self.citizens.get(f)) {
            Some(c) => c.name.rsplit(' ').next().unwrap_or(&c.name).to_string(),
            None => crate::names::family_name(&mut crate::rng_for(h, 0)),
        };
        let taken: Vec<String> = self.estate.owners.values().map(|o| o.name.clone()).collect();
        let name = (0..8u64).map(|k| builder_name(&surname, crate::hash::mix(h, k))).find(|n| !taken.contains(n)).unwrap_or_else(|| format!("{surname} Builders {}", taken.len()));
        let o = self.new_owner(name.clone(), OwnerKind::Builder { founder: founder.unwrap_or_default() });
        let who = founder.and_then(|f| self.citizens.get(f)).map_or("a local".to_string(), |c| c.name.clone());
        self.note_owner(o, format!("founded by {who} to build and let homes"));
        if let Some(f) = founder {
            let tick = self.clock.tick;
            if let Some(c) = self.citizens.get_mut(f) {
                c.remember(tick, format!("Started my own building firm: {name}."));
            }
        }
        o
    }

    // ── the day ────────────────────────────────────────────────────────────

    /// Once a day, before the owners settle: the tower developers run from
    /// town, old developers sell up, builders grow up and merge, fleets get
    /// their colours.
    pub(in crate::world) fn daily_companies(&mut self) {
        crate::prof_scope!("daily_companies");
        self.localise_tower_developers();
        self.retire_developers();
        self.rename_grown_builders();
        self.merge_builders();
        self.assign_liveries();
    }

    /// [companies] The tower league's developers are local firms: one with
    /// no founder (a new one, or an old save's out-of-town name) is taken
    /// on by a resident and named after them; its owner record becomes a
    /// builder.
    pub(in crate::world) fn localise_tower_developers(&mut self) {
        if self.citizens.is_empty() {
            return;
        }
        for i in 0..self.skyline.developers.len() {
            let (old, first, owner) = {
                let d = &self.skyline.developers[i];
                if d.founder.is_some_and(|f| self.citizens.contains_key(f)) {
                    continue;
                }
                (d.name.clone(), d.founder.is_none(), d.owner.filter(|o| self.estate.owners.contains_key(*o)))
            };
            let Some(f) = self.found_builder_founder(self.city_centre()) else { continue };
            let who = self.citizens[f].name.clone();
            let surname = who.rsplit(' ').next().unwrap_or("Local").to_string();
            let h = crate::hash::mix(self.config.seed ^ 0x70_3E, i as u64);
            let taken: Vec<String> = self.estate.owners.values().map(|o| o.name.clone()).chain(self.skyline.developers.iter().map(|d| d.name.clone())).collect();
            // A firm whose founder died is taken over under its own name.
            let name = if first { (0..8u64).map(|k| tower_firm_name(&surname, crate::hash::mix(h, k))).find(|n| !taken.contains(n)).unwrap_or_else(|| format!("{surname} Towers {i}")) } else { old.clone() };
            let dev = &mut self.skyline.developers[i];
            dev.founder = Some(f);
            dev.name = name.clone();
            match owner.map(|o| (o, self.estate.owners[o].kind.clone())) {
                // An old save's out-of-town record (which bought ordinary
                // blocks too) is left to sell up like the other developers;
                // the local firm gets a record of its own with its next
                // tower.
                Some((o, OwnerKind::Developer)) => {
                    self.skyline.developers[i].owner = None;
                    self.note_owner(o, format!("its tower business taken over by {name}, run from town by {who}"));
                }
                Some((o, _)) => {
                    let ow = &mut self.estate.owners[o];
                    ow.kind = OwnerKind::Builder { founder: f };
                    ow.name = name;
                    ow.grown = true;
                    self.note_owner(o, format!("taken over by {who}"));
                }
                None => {}
            }
        }
    }

    /// The resident a new local firm near `at` would be founded by.
    /// Nobody whose family already runs a builder (one family does not
    /// found every firm in town).
    fn found_builder_founder(&self, at: Vec2) -> Option<CitizenId> {
        let taken: Vec<CitizenId> = self.estate.owners.values().filter_map(|o| if let OwnerKind::Builder { founder } = o.kind { Some(founder) } else { None }).chain(self.skyline.developers.iter().filter_map(|d| d.founder)).collect();
        let families: Vec<&str> = taken.iter().filter_map(|f| self.citizens.get(*f)).map(|c| c.name.rsplit(' ').next().unwrap_or("")).collect();
        let homes: Vec<HouseholdId> = taken.iter().filter_map(|f| self.citizens.get(*f)?.household).collect();
        self.citizens
            .iter()
            .filter(|(id, c)| c.stage() == crate::citizens::LifeStage::Adult && !taken.contains(id) && !self.estate.is_elite(*id))
            .filter(|(_, c)| !c.household.is_some_and(|h| homes.contains(&h)) && !families.contains(&c.name.rsplit(' ').next().unwrap_or("")))
            .filter_map(|(id, c)| c.home.and_then(|h| self.building_lot(h)).map(|l| (id, l.center.distance(at), self.wealth_of(c))))
            .max_by(|a, b| (a.2 - a.1 * 0.2).total_cmp(&(b.2 - b.1 * 0.2)).then(b.0.cmp(&a.0)))
            .map(|(id, _, _)| id)
    }

    /// [companies] Old saves: the out-of-town developers no longer buy;
    /// each sells `RETIRE_PER_DAY` of its buildings a day, oldest first, to
    /// the local builder that would have bought there (founding new ones
    /// as the town has room), at the sale price (from the bank, out of
    /// town, as every sale). Owning nothing, it leaves (`prune_estate`).
    pub(in crate::world) fn retire_developers(&mut self) {
        let devs: Vec<OwnerId> = self.estate.owners.iter().filter(|(_, o)| o.kind == OwnerKind::Developer && !o.properties.is_empty()).map(|(id, _)| id).collect();
        if devs.is_empty() {
            return;
        }
        let mut rng = self.erng(super::estate::erng_salt::RETIRE);
        for d in devs {
            let props: Vec<BuildingId> = self.estate.owners[d].properties.iter().copied().take(RETIRE_PER_DAY).collect();
            let dname = self.estate.owners[d].name.clone();
            let mut sold = 0;
            for bid in props {
                let Some(lot) = self.buildings.get(bid).map(|b| b.lot) else { continue };
                let buyer = self.builder_for(lot, &mut rng);
                let price = self.sale_price(bid);
                self.settle_sale_to(bid, Some(buyer));
                self.give_to(bid, buyer, price);
                let bname = self.buildings[bid].name.clone();
                self.note_owner(buyer, format!("bought {bname} from {dname}"));
                self.events.push(SimEvent::BuildingChanged(bid));
                sold += 1;
            }
            if sold > 0 {
                let left = self.estate.owners.get(d).map_or(0, |o| o.properties.len());
                self.note_owner(d, format!("sold {sold} buildings to local firms, {left} left"));
                if left == 0 {
                    let tick = self.clock.tick;
                    let pos = self.city_centre();
                    self.skyline.push_news(tick, format!("{dname} has sold its last building in town to local firms"), pos, None);
                }
            }
        }
    }

    /// [companies] A builder past `BUILDER_GROWN` properties takes a grown-up
    /// name, once.
    pub(in crate::world) fn rename_grown_builders(&mut self) {
        let grown: Vec<OwnerId> = self.estate.owners.iter().filter(|(_, o)| matches!(o.kind, OwnerKind::Builder { .. }) && !o.grown && o.properties.len() >= BUILDER_GROWN).map(|(id, _)| id).collect();
        for oid in grown {
            let old = self.estate.owners[oid].name.clone();
            let h = crate::hash::mix(self.config.seed ^ 0x6E_0E, oid.data().as_ffi());
            let taken: Vec<String> = self.estate.owners.values().map(|o| o.name.clone()).collect();
            let surname = surname_of(&old).to_string();
            let Some(name) = (0..8u64).map(|k| grown_name(&surname, crate::hash::mix(h, k))).find(|n| !taken.contains(n)) else { continue };
            let o = &mut self.estate.owners[oid];
            o.name = name.clone();
            o.grown = true;
            let n = o.properties.len();
            self.note_owner(oid, format!("renamed from {old}, with {n} buildings"));
            let tick = self.clock.tick;
            let pos = self.owner_centre(oid).unwrap_or_default();
            self.skyline.push_news(tick, format!("{old} has grown to {n} buildings and is now {name}"), pos, None);
        }
    }

    /// [companies] One merger a day at most (a hash roll): a big builder
    /// buys out a small one (at most `MERGE_MAX_SMALL` buildings, a
    /// `MERGE_RATIO`th of its size) whose holdings lie within
    /// `MERGE_REACH`. The buildings and the books (money or debt) go to the
    /// buyer; the founder is paid `MERGE_PRICE_SHARE` of the buildings'
    /// value out of the buyer's account.
    pub(in crate::world) fn merge_builders(&mut self) {
        let roll = crate::hash::unit(crate::hash::mix(self.config.seed ^ 0x3E_76, self.clock.day()));
        if roll >= 0.35 {
            return;
        }
        let firms: Vec<(OwnerId, usize, Vec2)> = self.estate.owners.iter().filter(|(_, o)| matches!(o.kind, OwnerKind::Builder { .. }) && !o.properties.is_empty()).filter_map(|(id, o)| self.owner_centre(id).map(|c| (id, o.properties.len(), c))).collect();
        // Tower developers keep their own records.
        let towers: Vec<OwnerId> = self.skyline.developers.iter().filter_map(|d| d.owner).collect();
        let mut best: Option<(f32, OwnerId, OwnerId)> = None;
        for &(small, n, at) in &firms {
            if n > MERGE_MAX_SMALL || towers.contains(&small) {
                continue;
            }
            for &(big, m, bt) in &firms {
                if big == small || m < MERGE_RATIO * n.max(1) {
                    continue;
                }
                let d = at.distance(bt);
                if d <= MERGE_REACH && best.is_none_or(|(bd, bs, bb)| d < bd || (d == bd && (small, big) < (bs, bb))) {
                    best = Some((d, small, big));
                }
            }
        }
        let Some((_, small, big)) = best else { return };
        let (sname, bname) = (self.estate.owners[small].name.clone(), self.estate.owners[big].name.clone());
        let props = self.estate.owners[small].properties.clone();
        let value: f64 = props.iter().map(|p| self.sale_price(*p)).sum();
        for p in &props {
            self.give_to(*p, big, 0.0);
            self.events.push(SimEvent::BuildingChanged(*p));
        }
        // The books go with the firm: its money (or its debt).
        let money = self.economy.bank.balance(Account::Owner(small));
        self.transfer64(Account::Owner(small), Account::Owner(big), money, Kind::Investment);
        if let Some(f) = self.estate.owners[small].kind.founder().filter(|f| self.citizens.contains_key(*f)) {
            let to = self.purse_of(f);
            self.transfer64(Account::Owner(big), to, value * MERGE_PRICE_SHARE, Kind::PropertySale);
        }
        self.note_owner(big, format!("bought out {sname} ({} buildings)", props.len()));
        self.note_owner(small, format!("bought out by {bname}"));
        let tick = self.clock.tick;
        let pos = self.owner_centre(big).unwrap_or_default();
        self.skyline.push_news(tick, format!("{bname} buys out rival builder {sname}"), pos, None);
    }

    // ── growing ────────────────────────────────────────────────────────────

    /// [companies] The next step a shop or a fleet yard doing well takes:
    /// the plot next door while its site is under its cap (a shop's
    /// `MAX_MERGED_LOTS`, a yard's `YARD_MAX_LOTS`) and a neighbour is for
    /// sale, else another store or depot across town. `None` for other
    /// trades. [G2] Malls come in here (`ShopGrowth::Mall`).
    pub fn shop_growth(&self, bid: BuildingId) -> Option<ShopGrowth> {
        let b = self.buildings.get(bid)?;
        let kind = b.business.as_ref()?.kind;
        let cap = if shop_chain(kind) {
            MAX_MERGED_LOTS
        } else if yard_trade(kind) {
            YARD_MAX_LOTS
        } else {
            return None;
        };
        let ours = b.plot.lots(b.lot);
        let lu = self.lots.get(b.lot).map_or(b.land_use, |l| l.land_use);
        // [malls] A store whose plots make a big enough site rebuilds as a
        // mall; a mall's company grows by opening stores across town (the
        // mall itself grows into a mega-mall on its own: `grow_malls`).
        if shop_chain(kind) && self.mall_site_ok(bid) {
            return Some(ShopGrowth::Mall);
        }
        if b.plot.form.is_some_and(|f| f.is_mall()) {
            return Some(ShopGrowth::Branch);
        }
        if ours.len() < cap && b.plot.tower.is_none() && !self.plots_next_to(&ours, lu, b.floors.saturating_add(1), b.plot.owner).is_empty() {
            return Some(ShopGrowth::Neighbour);
        }
        Some(ShopGrowth::Branch)
    }

    /// [companies] A firm doing well grows one step: a shop or a yard by
    /// `shop_growth` (next door, then across town), a branch trade by a
    /// branch, the rest into the plots next door. True if it grew.
    pub fn grow_firm(&mut self, bid: BuildingId) -> bool {
        let Some(kind) = self.buildings.get(bid).and_then(|b| b.business.as_ref()).map(|z| z.kind) else { return false };
        if let Some(step) = self.shop_growth(bid) {
            let grew = match step {
                ShopGrowth::Neighbour => self.expand_into_neighbours(bid) || self.open_branch(bid).is_some(),
                ShopGrowth::Branch => self.open_branch(bid).is_some(),
                ShopGrowth::Mall => self.build_mall(bid),
            };
            if grew {
                if let Some(o) = self.buildings.get(bid).and_then(|b| b.plot.owner) {
                    let n = self.estate.owners.get(o).map_or(0, |o| o.properties.len());
                    let lots = self.buildings.get(bid).map_or(1, |b| 1 + b.plot.extra_lots.len() + b.plot.pending_lots.len());
                    let what = match step {
                        ShopGrowth::Neighbour if lots > 1 => format!("bought the plot next door ({lots} plots)"),
                        ShopGrowth::Mall => format!("is rebuilding its {} as a mall ({lots} plots)", kind.label()),
                        _ => format!("opened another {} ({n} in all)", kind.label()),
                    };
                    self.note_owner(o, what);
                }
            }
            return grew;
        }
        if branches(kind) {
            self.open_branch(bid).is_some()
        } else if expansion_form(kind).is_some() {
            self.expand_into_neighbours(bid)
        } else {
            false
        }
    }

    // ── fleet liveries ─────────────────────────────────────────────────────

    /// Where a business's colour lives: its owner record when it has one
    /// of its own (a firm, a founder, a property company, a builder),
    /// else the business itself. `None` for corps (their brand) and owners
    /// that do not run it (the university, a developer, a family).
    fn livery_holder(&self, bid: BuildingId) -> Option<Result<OwnerId, BuildingId>> {
        let b = self.buildings.get(bid)?;
        match b.plot.owner.and_then(|o| self.estate.owners.get(o).map(|x| (o, x))) {
            Some((o, x)) => match x.kind {
                OwnerKind::Firm { .. } | OwnerKind::Person { .. } | OwnerKind::Company { .. } | OwnerKind::Builder { .. } => Some(Ok(o)),
                // A billionaire's or a family's business keeps its own.
                OwnerKind::Elite { .. } | OwnerKind::Household { .. } => b.business.as_ref().map(|_| Err(bid)),
                OwnerKind::Developer | OwnerKind::MegaCorp { .. } | OwnerKind::University { .. } => None,
            },
            None => b.business.as_ref().map(|_| Err(bid)),
        }
    }

    /// [companies] A local company's fleet colour for the vehicles of the
    /// business at `bid`, if it has one.
    pub fn fleet_colour(&self, bid: BuildingId) -> Option<u8> {
        match self.livery_holder(bid)? {
            Ok(o) => self.estate.owners.get(o)?.livery,
            Err(b) => self.buildings.get(b)?.business.as_ref()?.livery,
        }
    }

    /// [companies] The livery the vehicles of the business at `bid` wear:
    /// a corp's brand, else its own company's colour over `base` (the
    /// trade's livery); `None` to keep `base`.
    pub fn fleet_livery(&self, bid: BuildingId, base: Livery) -> Option<Livery> {
        self.corp_livery(bid).or_else(|| self.fleet_colour(bid).map(|c| Livery::fleet(base, c)))
    }

    /// [companies] A builder's colour for the lorries to its building site.
    pub fn builder_colour(&self, site: BuildingId) -> Option<u8> {
        let o = self.estate.owners.get(self.buildings.get(site)?.plot.owner?)?;
        matches!(o.kind, OwnerKind::Builder { .. }).then_some(o.livery).flatten()
    }

    /// [companies] Every local company with vehicles gets a colour: one
    /// that no rival of the same trade within `LIVERY_RIVAL_REACH` wears,
    /// from a hash-picked start. Builders count as a trade of their own.
    /// Cabs already on the books are repainted to match.
    pub(in crate::world) fn assign_liveries(&mut self) {
        // (holder, trade, where) of everyone with a fleet.
        let mut fleets: Vec<(Result<OwnerId, BuildingId>, u8, Vec2)> = Vec::new();
        for (bid, b) in self.buildings.iter() {
            let Some(z) = &b.business else { continue };
            // The city's own (a cargo hub) wear the city's colours.
            if z.closed || !runs_fleet(z.kind) || b.is_city_structure() {
                continue;
            }
            let Some(h) = self.livery_holder(bid) else { continue };
            let Some(at) = self.lots.get(b.lot).map(|l| l.center) else { continue };
            fleets.push((h, z.kind.index() as u8, at));
        }
        for (oid, o) in self.estate.owners.iter() {
            if matches!(o.kind, OwnerKind::Builder { .. }) {
                if let Some(at) = self.owner_centre(oid) {
                    fleets.push((Ok(oid), u8::MAX, at));
                }
            }
        }
        let colour_of = |w: &World, h: &Result<OwnerId, BuildingId>| -> Option<u8> {
            match h {
                Ok(o) => w.estate.owners.get(*o).and_then(|o| o.livery),
                Err(b) => w.buildings.get(*b).and_then(|b| b.business.as_ref()).and_then(|z| z.livery),
            }
        };
        let n = FLEET_COLOURS.len() as u64;
        for i in 0..fleets.len() {
            let (h, trade, at) = fleets[i];
            if colour_of(self, &h).is_some() {
                continue;
            }
            let used: Vec<u8> = fleets.iter().filter(|(x, t, p)| *x != h && *t == trade && p.distance(at) <= LIVERY_RIVAL_REACH).filter_map(|(x, _, _)| colour_of(self, x)).collect();
            let key = match h {
                Ok(o) => o.data().as_ffi(),
                Err(b) => b.data().as_ffi() ^ 0xB1,
            };
            let start = crate::hash::mix(self.config.seed ^ 0x11_7E, key) % n;
            let c = (0..n).map(|k| ((start + k * 5) % n) as u8).find(|c| !used.contains(c)).unwrap_or(start as u8);
            match h {
                Ok(o) => {
                    self.estate.owners[o].livery = Some(c);
                }
                Err(b) => {
                    if let Some(z) = self.buildings.get_mut(b).and_then(|b| b.business.as_mut()) {
                        z.livery = Some(c);
                    }
                }
            }
        }
        // Cabs on the books (and on the road) in their company's colours.
        let cabs: Vec<(crate::taxi::CabId, BuildingId)> = self.taxis.cabs.iter().map(|(id, c)| (id, c.company)).collect();
        for (id, company) in cabs {
            let want = self.fleet_livery(company, Livery::Taxi).unwrap_or(Livery::Taxi);
            let cab = &mut self.taxis.cabs[id];
            if cab.model.livery != want {
                cab.model.livery = want;
                if let crate::taxi::CabPlace::Road(car) = cab.place {
                    if let Some(c) = self.traffic.cars.get_mut(car) {
                        c.model.livery = want;
                    }
                }
            }
        }
    }

    // ── the directory ──────────────────────────────────────────────────────

    /// [companies] Vehicles each business runs now: its cabs, tow trucks,
    /// garbage trucks, and lorries and vans on the road from it.
    pub fn fleet_counts(&self) -> BTreeMap<BuildingId, usize> {
        let mut m: BTreeMap<BuildingId, usize> = BTreeMap::new();
        for c in self.taxis.cabs.values() {
            *m.entry(c.company).or_default() += 1;
        }
        for t in self.society.trash.trucks.values() {
            if let crate::trash::Hauler::Company(b) = t.hauler {
                *m.entry(b).or_default() += 1;
            }
        }
        for c in self.traffic.cars.values() {
            if let Some(g) = &c.cargo {
                if !g.from.is_null() {
                    *m.entry(g.from).or_default() += 1;
                }
            }
        }
        for (id, b) in self.buildings.iter() {
            if b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::TowYard) {
                *m.entry(id).or_default() += self.tow_trucks(id);
            }
        }
        m
    }

    /// [companies] Every owner as a row of the company directory: what it
    /// is, its founder, properties, their value, rent and takings
    /// yesterday, staff, vehicles, money and history; biggest first by
    /// `size` (value plus a year of rent and takings).
    pub fn company_summaries(&self) -> Vec<CompanySummary> {
        let fleets = self.fleet_counts();
        let mut out: Vec<CompanySummary> = Vec::new();
        for (oid, o) in self.estate.owners.iter() {
            let (kind_key, what) = owner_kind_words(self, oid, &o.kind);
            let held: Vec<(BuildingId, &Building)> = o.properties.iter().filter_map(|p| self.buildings.get(*p).map(|b| (*p, b))).collect();
            // Its businesses: its buildings' own, and the tenants in them
            // that are its (a corp renting itself a floor is rare; tenants
            // pay it rent, counted as rent).
            let value: f64 = held.iter().map(|(p, _)| self.sale_price(*p)).sum();
            let takings: f32 = held.iter().filter_map(|(_, b)| b.business.as_ref()).filter_map(|z| z.books.last()).map(|d| d.revenue.max(0.0)).sum::<f32>() + 0.0;
            let staff: usize = held.iter().filter(|(_, b)| b.operating()).map(|(_, b)| b.workers.len()).sum();
            let vehicles: usize = held.iter().map(|(p, _)| fleets.get(p).copied().unwrap_or(0)).sum();
            let founder = match &o.kind {
                OwnerKind::Household { household } => self.households.get(*household).and_then(|h| h.members.first()).and_then(|m| self.citizens.get(*m)).map(|c| c.name.clone()),
                k => k.founder().and_then(|f| self.citizens.get(f)).map(|c| c.name.clone()),
            };
            let money = match o.kind {
                OwnerKind::MegaCorp { .. } => self.corps.corp_of_owner(oid).map_or(0.0, |c| c.cash),
                OwnerKind::University { .. } => self.corps.campus_of_owner(oid).map_or(0.0, |c| c.funds),
                OwnerKind::Elite { citizen } => self.estate.elite_of(citizen).map_or(0.0, |e| e.net_worth),
                _ => o.money,
            };
            let hq = self.corps.corp_of_owner(oid).and_then(|c| c.hq).or_else(|| self.corps.campus_of_owner(oid).map(|k| k.building));
            let flagship = hq.filter(|b| self.buildings.contains_key(*b)).or_else(|| held.iter().max_by(|a, b| (a.1.homes as u32 + a.1.jobs as u32).cmp(&(b.1.homes as u32 + b.1.jobs as u32)).then(b.0.cmp(&a.0))).map(|(p, _)| *p));
            let livery = match o.kind {
                OwnerKind::MegaCorp { .. } => None,
                _ => o.livery.or_else(|| held.iter().find_map(|(_, b)| b.business.as_ref().and_then(|z| z.livery))),
            };
            out.push(CompanySummary {
                owner: oid,
                name: o.name.clone(),
                what,
                kind_key,
                founder,
                properties: held.len(),
                value,
                rent: o.rent_yesterday,
                takings,
                staff,
                vehicles,
                money,
                since_day: o.since_day,
                history: o.history.clone(),
                livery,
                flagship,
                size: value + 365.0 * (o.rent_yesterday as f64 + takings as f64),
            });
        }
        out.sort_by(|a, b| b.size.total_cmp(&a.size).then(b.properties.cmp(&a.properties)).then(a.owner.cmp(&b.owner)));
        out
    }

    /// [companies] "Biggest companies": every owner that is a company or
    /// owns like one (families in their own house left out), biggest first.
    pub fn biggest_companies(&self, n: usize) -> Vec<CompanySummary> {
        self.company_summaries().into_iter().filter(|c| c.kind_key != "family" && c.properties > 0).take(n).collect()
    }

    /// [companies] What corp `ci` knows and builds (`MegaCorp::specialty`).
    pub fn corp_specialty(&self, ci: usize) -> Specialty {
        self.corps.corps.get(ci).map_or(Specialty::Property, |c| c.specialty())
    }
}
