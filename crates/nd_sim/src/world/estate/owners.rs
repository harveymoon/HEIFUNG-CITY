//! [land] Prices and owners: what a building sells for, who owns it, sales,
//! reservations and plots changing hands.

use super::*;

impl World {
    /// [review] What the buildings standing on `plots` sell for, summed
    /// (`sale_price` each; a vacant plot costs nothing here).
    pub(in crate::world) fn plots_price(&self, plots: &[LotId]) -> f64 {
        plots.iter().filter_map(|l| self.lots.get(*l).and_then(|x| x.building)).map(|b| self.sale_price(b)).sum()
    }

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
    pub(super) fn value_of_new(&self, bid: BuildingId, tier: Tier, floors: u8) -> f64 {
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
            // [ledger] A resident's holdings sit on more than one record
            // (their own, their household's, their property company's).
            let person = match &o.kind {
                OwnerKind::Person { citizen } | OwnerKind::Elite { citizen } => Some(*citizen),
                OwnerKind::Company { founder } | OwnerKind::Builder { founder } => Some(*founder),
                OwnerKind::Household { household } => self.households.get(*household).and_then(|h| h.members.first().copied()),
                _ => None,
            };
            match person.filter(|c| self.citizens.contains_key(*c)) {
                Some(c) => out.extend(self.properties_of(c).into_iter().filter(|p| *p != bid)),
                None => out.extend(o.properties.iter().copied().filter(|p| *p != bid && self.buildings.contains_key(*p))),
            }
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
        // [land-assembly] An open-ground site (a pit, fields, a car park)
        // is drawn on the plots it owns, joined into one outline, however
        // ragged; it is framed and entered by its road plot.
        let open = b.plot.form == Some(Form::Merged) && b.business.as_ref().is_some_and(|z| crate::estate::grows_in_place(z.kind));
        let slack = match b.plot.form {
            Some(Form::Tower) => crate::towers::TOWER_HULL_SLACK,
            // [malls] A mega-mall's car park fills the corners of its block.
            Some(Form::MegaMall) => super::super::malls::MALL_HULL_SLACK,
            _ if open => crate::estate::SITE_HULL_SLACK,
            _ => MERGE_HULL_SLACK,
        };
        Some(match merged_lot_within(lot, &others, slack) {
            Some(mut m) if open => {
                let polys: Vec<Vec<Vec2>> = std::iter::once(lot).chain(others.iter().copied()).map(|l| l.polygon.clone()).collect();
                if let Some(u) = crate::zoning::joined_outline(&polys).filter(|u| u.len() >= 3) {
                    m.center = crate::zoning::inner_center(&u);
                    m.polygon = u;
                }
                Cow::Owned(m)
            }
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

    pub(in crate::world) fn new_owner(&mut self, name: String, kind: OwnerKind) -> OwnerId {
        let day = self.clock.day();
        self.estate.owners.insert(Owner { name, kind, properties: Vec::new(), spent: 0.0, since_day: day as u32, income_day: 0.0, earned: 0.0, money: 0.0, rent_today: 0.0, rent_yesterday: 0.0, livery: None, history: Vec::new(), grown: false })
    }

    /// `owner` buys `bid` (or takes it on as built): off its old owner's
    /// books, onto the new one's.
    pub(in crate::world) fn give_to(&mut self, bid: BuildingId, owner: OwnerId, price: f64) {
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

    /// [review] The market buys `bid` from whoever owns it: the owner is
    /// paid its [`World::sale_price`] (what the buyer's price was summed
    /// from) and lets it go. Sellers were never paid: an investor's second
    /// home, bought with a real deposit, could be taken for nothing. Called
    /// after `pack_business` (a business that moves takes its owner and
    /// goodwill along) and before the building is pulled down or rebuilt.
    pub(in crate::world) fn settle_sale(&mut self, bid: BuildingId) {
        self.settle_sale_to(bid, None);
    }

    /// [`World::settle_sale`] to `buyer`, when known. [ledger] An owner
    /// "selling" to itself (a developer rebuilding its own block taller, a
    /// firm taking its own plot next door) is paid nothing: every such
    /// rebuild booked the building's full price from out of town into the
    /// developer, which sent it straight back out as profit (millions a
    /// day on a big city's money-flow page, `e2-tenants`).
    pub(in crate::world) fn settle_sale_to(&mut self, bid: BuildingId, buyer: Option<OwnerId>) {
        let Some(oid) = self.buildings.get(bid).and_then(|b| b.plot.owner) else { return };
        let price = if buyer == Some(oid) { 0.0 } else { self.sale_price(bid) };
        self.buildings[bid].plot.owner = None;
        let Some(o) = self.estate.owners.get_mut(oid) else { return };
        o.properties.retain(|p| *p != bid);
        if price <= 0.0 {
            return;
        }
        o.earned += price;
        // [ledger] Paid by the buyer, from out of town, into the seller's
        // account: a resident's purse, a corp's cash, a firm's, developer's
        // or company's own money, the university's funds.
        let to = self.owner_account(oid);
        self.transfer64(Account::Outside, to, price, Kind::PropertySale);
    }

    /// The owner record that is `cid`'s own: their billionaire record, a
    /// person record, or the household's (a McMansion), made on first use.
    pub(in crate::world) fn owner_for_citizen(&mut self, cid: CitizenId) -> Option<OwnerId> {
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
            .filter(|o| o.kind.citizen() == Some(cid) || o.kind == OwnerKind::Company { founder: cid } || o.kind == OwnerKind::Builder { founder: cid } || hh.is_some_and(|h| o.kind == OwnerKind::Household { household: h }))
            .flat_map(|o| o.properties.iter().copied())
            .filter(|b| self.buildings.contains_key(*b))
            .collect();
        out.extend(self.buildings.iter().filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.owner == Some(cid))).map(|(id, _)| id));
        out.sort();
        out.dedup();
        out
    }

    /// The owner record of a firm, made on its first branch or merge.
    pub(in crate::world) fn firm_of(&mut self, bid: BuildingId) -> Option<OwnerId> {
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

    /// Can the market buy this building? Built and standing, ordinary (not
    /// the city's, a car park, a taxi yard or cargo hub), not in a frozen
    /// historic district, not a home bought outright, not a billionaire's.
    pub(in crate::world) fn buyable(&self, bid: BuildingId) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        if !b.operating() || !b.is_grown() || b.land_use == LandUse::Park || b.land_use == LandUse::Civic {
            return false;
        }
        if b.business.as_ref().is_some_and(|z| matches!(z.kind, BusinessKind::TaxiCompany | BusinessKind::CargoHub)) {
            return false;
        }
        // A home bought outright is not for sale while its owners hold it
        // ([review] an empty mansion, its billionaire dead, is).
        if b.plot.form.is_some_and(|f| f.owner_occupied()) && b.plot.owner.is_some() || !b.plot.pending_lots.is_empty() {
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

    /// A building is gone (removed outright or cleared after demolition):
    /// its extra plots are free again, it leaves its owner's books, and any
    /// plot it stood on that someone bought is handed over.
    pub(in crate::world) fn estate_building_gone(&mut self, bid: BuildingId, plot: &Plot, lot: LotId) {
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

    /// A reservation on `lot` is dropped (something else takes the plot):
    /// the reserving building forgets it as pending.
    pub(in crate::world) fn drop_reservation(&mut self, lot: LotId) {
        if let Some(bid) = self.estate.reserved.remove(&lot) {
            if let Some(b) = self.buildings.get_mut(bid) {
                b.plot.pending_lots.retain(|l| *l != lot);
            }
        }
    }

    /// A plot someone bought is clear: it becomes part of their building.
    pub(in crate::world) fn claim_reserved(&mut self, lot: LotId) {
        let Some(&bid) = self.estate.reserved.get(&lot) else { return };
        if self.lots.get(lot).is_none_or(|l| l.building.is_some()) {
            if self.lots.get(lot).is_none() {
                self.estate.reserved.remove(&lot);
            }
            return;
        }
        self.estate.reserved.remove(&lot);
        // [review] A plot a campus bought for an annex gets the annex, not a
        // share of the university building (it stayed pending forever).
        if let Some((ki, annex)) = self.corps.campuses.iter().enumerate().find_map(|(ki, k)| k.pending.iter().find(|(l, _)| *l == lot).map(|(_, a)| (ki, *a))) {
            self.corps.campuses[ki].pending.retain(|(x, _)| *x != lot);
            self.build_annex(ki, lot, annex);
            return;
        }
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
    pub(in crate::world) fn take_plots(&mut self, bid: BuildingId, plots: &[LotId], memory: &str) {
        self.take_plots_for(bid, plots, memory, crate::buildings::DemolitionReason::Expansion);
    }

    /// [pt-growth] `take_plots`, saying why the buildings on them come down.
    pub(in crate::world) fn take_plots_for(&mut self, bid: BuildingId, plots: &[LotId], memory: &str, reason: crate::buildings::DemolitionReason) {
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
                    let buyer = self.buildings.get(bid).and_then(|b| b.plot.owner);
                    self.settle_sale_to(other, buyer);
                    // A park standing on rezoned ground just goes.
                    self.estate.reserved.insert(l, bid);
                    if let Some(b) = self.buildings.get_mut(bid) {
                        b.plot.pending_lots.push(l);
                    }
                    self.demolish_with(other, None, Some(memory));
                    self.tag_demolition(other, reason);
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
    pub(in crate::world) fn plots_next_to(&self, ours: &[LotId], lu: LandUse, max_floors: u8, owner: Option<OwnerId>) -> Vec<(f64, LotId)> {
        let mine: Vec<&Lot> = ours.iter().filter_map(|l| self.lots.get(*l)).collect();
        let mut out: Vec<(f64, LotId)> = Vec::new();
        for (lid, l) in self.lots.iter() {
            if l.land_use != lu || ours.contains(&lid) || !self.site_clear(lid, SiteRule::ANY) {
                continue;
            }
            // [r4-decks] A plot under a raised road joins no bigger building.
            if !mine.iter().any(|m| m.center.distance(l.center) < 120.0 && lots_touch(m, l)) || self.under_a_deck(lid) {
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
}
