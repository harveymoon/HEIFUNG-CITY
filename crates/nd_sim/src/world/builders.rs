//! [r14-builders] Builder companies and the builders' merchant, the
//! `World` side (`crate::builders` has the numbers, names and books).
//!
//! - [`World::tender_site`]: a site's job goes to a contractor in town with
//!   a crew free, else to a firm from out of town.
//! - [`World::buy_site_load`]: the delivery being ordered, line by line of
//!   the site's bill ([`World::site_load`]): the builder's yard, a scrap
//!   yard, the cheapest of the merchant and the makers, else an import.
//! - [`World::builders_hourly`]: merchants restock their trade stock,
//!   contractors fill their yards in the morning, the crews' sites list.
//! - [`World::builders_daily`]: the books, and the builders gone bust.
//! - [`World::crew_site`]: where a contractor's worker goes to work today.

use super::market::TruckOrigin;
use super::*;
use crate::builders::{self as bld, BuildersDay};
use crate::buildings::{SiteBuilder, SiteContract, SiteLoad};

/// Colour index of the lorries the builders send (the trade livery is put
/// on in `dispatch_truck`).
const SITE_LORRY_COLOR: u8 = 7;

impl World {
    // ── reading ─────────────────────────────────────────────────────────────

    /// The site's bill for the delivery it is ordering now: a list of
    /// (good, quantity), the stage's lines still to order
    /// ([`World::site_lines_to_order`], r14-materials' bill by build type).
    pub fn site_load(&self, bid: BuildingId) -> Vec<(Resource, f32)> {
        self.site_lines_to_order(bid).into_iter().map(|l| (l.good, l.qty)).collect()
    }

    /// A contractor's yard that is open for work.
    pub fn is_contractor(&self, bid: BuildingId) -> bool {
        self.buildings.get(bid).is_some_and(|b| b.operating() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Contractor && !z.closed))
    }

    /// The crews a contractor runs (its staff in crews of `CREW_SIZE`).
    pub fn contractor_crews(&self, bid: BuildingId) -> u16 {
        self.buildings.get(bid).map_or(0, |b| bld::crews(b.workers.len()))
    }

    /// The sites a contractor is building now.
    pub fn contractor_sites(&self, bid: BuildingId) -> &[BuildingId] {
        self.builder_sites.get(&bid).map_or(&[], |v| v.as_slice())
    }

    /// Who is building site `bid`: the name, and the contractor's yard
    /// when it is a firm in town.
    pub fn site_builder(&self, bid: BuildingId) -> Option<(String, Option<BuildingId>)> {
        let k = self.buildings.get(bid)?.construction.as_ref()?.contract.as_ref()?;
        Some(match &k.builder {
            SiteBuilder::Local(c) => (self.buildings.get(*c).map_or_else(|| "a builder since gone".to_string(), |b| b.name.clone()), Some(*c)),
            SiteBuilder::OutOfTown { name, .. } => (name.clone(), None),
        })
    }

    /// The colour of the site's builder's vans (a `FLEET_COLOURS` index)
    /// and whether it is a firm in town, for the site's look.
    pub fn site_builder_livery(&self, bid: BuildingId) -> Option<(u8, bool)> {
        let k = self.buildings.get(bid)?.construction.as_ref()?.contract.as_ref()?;
        match &k.builder {
            SiteBuilder::Local(c) => Some((self.fleet_colour(*c).unwrap_or(crate::hash::mix(slotmap::Key::data(c).as_ffi(), 0xC0) as u8 % 16), true)),
            SiteBuilder::OutOfTown { livery, .. } => Some((*livery, false)),
        }
    }

    /// The crews' sites, by contractor: rebuilt from the sites' contracts.
    pub(super) fn refresh_builder_sites(&mut self) {
        let mut m: BTreeMap<BuildingId, Vec<BuildingId>> = BTreeMap::new();
        for (id, b) in self.buildings.iter() {
            if b.demolition.is_some() {
                continue;
            }
            if let Some(c) = b.construction.as_ref().and_then(|c| c.contract.as_ref()).and_then(|k| k.local()) {
                m.entry(c).or_default().push(id);
            }
        }
        self.builder_sites = m;
    }

    /// [r14-builders] Where contractor's worker `cid` goes to work today:
    /// one of its sites (picked by their seed, so a crew sticks to a site
    /// while the job lasts), or the yard when it has none.
    pub fn crew_site(&self, cid: CitizenId) -> Option<BuildingId> {
        let c = self.citizens.get(cid)?;
        let yard = c.work?;
        let sites = self.builder_sites.get(&yard)?;
        let live: Vec<BuildingId> = sites.iter().copied().filter(|s| self.buildings.get(*s).is_some_and(|b| b.construction.is_some() && b.demolition.is_none())).collect();
        if live.is_empty() {
            return None;
        }
        let i = (crate::hash::mix(c.seed, 0xC4E3) % live.len() as u64) as usize;
        Some(live[i])
    }

    // ── tendering ───────────────────────────────────────────────────────────

    /// What the job at `bid` should cost: its materials at the estimate's
    /// price for the whole bill, and its crew's payroll for the days.
    pub fn job_estimate(&self, bid: BuildingId) -> (f32, f32) {
        let Some(b) = self.buildings.get(bid) else { return (0.0, 0.0) };
        let Some(c) = &b.construction else { return (0.0, 0.0) };
        // The whole bill (every stage) at the estimate's price.
        let materials = crate::catalogue::bill_value(&self.site_bill(bid)) * bld::MATERIAL_ESTIMATE;
        let labour = Construction::crew(b.floors) as f32 * c.days * Building::SITE_WAGE * bld::LABOUR_SHARE;
        (materials, labour)
    }

    /// Sites each contractor has on now.
    fn contractor_load(&self) -> BTreeMap<BuildingId, u16> {
        self.builder_sites.iter().map(|(c, v)| (*c, v.len() as u16)).collect()
    }

    /// Does `bid` need a builder: no contract yet, or its contractor gone
    /// or shut?
    pub(super) fn needs_tender(&self, bid: BuildingId) -> bool {
        let Some(c) = self.buildings.get(bid).and_then(|b| b.construction.as_ref()) else { return false };
        if c.follows_park {
            return false;
        }
        match c.contract.as_ref().map(|k| &k.builder) {
            None => true,
            Some(SiteBuilder::Local(y)) => !self.is_contractor(*y),
            Some(SiteBuilder::OutOfTown { .. }) => false,
        }
    }

    /// Put the job at site `bid` out to tender: the nearest contractor in
    /// town with a crew free and the site within reach takes it, else a
    /// firm from out of town. A contractor's crew are its own staff: the
    /// site's labourers go (to the contractor, when it has room).
    pub fn tender_site(&mut self, bid: BuildingId) {
        if !self.needs_tender(bid) {
            return;
        }
        let Some(at) = self.building_lot(bid).map(|l| l.center) else { return };
        let load = self.contractor_load();
        let (materials, labour) = self.job_estimate(bid);
        // What is left to build: a job taken over half way costs the rest.
        let (left, deliveries_left) = self.buildings[bid].construction.as_ref().map_or((1.0, 4), |c| ((1.0 - c.progress).clamp(0.1, 1.0), Construction::DELIVERIES.saturating_sub(c.deliveries).max(1)));
        let estimate = (materials * deliveries_left as f32 / Construction::DELIVERIES as f32) + labour * left;
        let mut best: Option<(f32, BuildingId)> = None;
        for (id, b) in self.buildings.iter() {
            let Some(z) = &b.business else { continue };
            if z.kind != BusinessKind::Contractor || z.closed || !b.operating() {
                continue;
            }
            let crews = bld::crews(b.workers.len());
            let busy = load.get(&id).copied().unwrap_or(0);
            if crews == 0 || busy >= crews {
                continue;
            }
            let Some(l) = self.lots.get(b.lot) else { continue };
            let d = l.center.distance(at);
            if d > bld::LOCAL_REACH {
                continue;
            }
            // Nearer first; a contractor with crews idle bids keener.
            let score = d / 1000.0 + busy as f32 / crews as f32;
            if best.is_none_or(|(s, x)| score < s || (score == s && id < x)) {
                best = Some((score, id));
            }
        }
        let tick = self.clock.tick;
        let old_paid = 0.0;
        let contract = match best {
            Some((_, yard)) => {
                self.builders.today.let_local += 1;
                SiteContract { builder: SiteBuilder::Local(yard), price: estimate * bld::LOCAL_MARGIN, paid: old_paid, since: tick }
            }
            None => {
                self.builders.today.let_outside += 1;
                let seed = self.buildings[bid].seed;
                let h = crate::hash::mix(seed, 0x0B7_70E);
                SiteContract {
                    builder: SiteBuilder::OutOfTown { name: bld::out_of_town_name(h), livery: (crate::hash::mix(h, 0x11) % crate::companies::FLEET_COLOURS.len() as u64) as u8 },
                    price: materials * deliveries_left as f32 / Construction::DELIVERIES as f32 + labour * left * bld::OUT_OF_TOWN_MARKUP,
                    paid: old_paid,
                    since: tick,
                }
            }
        };
        // A job taken over keeps its start, for the build time.
        let since = self.buildings[bid].construction.as_ref().and_then(|c| c.contract.as_ref()).map(|k| k.since);
        let local = contract.local();
        let name = local.and_then(|y| self.buildings.get(y)).map(|b| b.name.clone());
        let b = &mut self.buildings[bid];
        if let Some(c) = b.construction.as_mut() {
            let mut k = contract;
            if let Some(s) = since {
                k.since = s;
            }
            c.contract = Some(k);
        }
        b.recompute_capacity();
        // A contractor's own crew: the labourers the site took on go, to
        // the contractor when it has jobs open.
        if let Some(yard) = local {
            let crew = std::mem::take(&mut self.buildings[bid].workers);
            let site_name = self.buildings[bid].name.clone();
            let firm = name.unwrap_or_default();
            for cid in crew {
                if self.citizens.get(cid).is_none_or(|c| c.work != Some(bid)) {
                    continue;
                }
                let room = self.buildings.get(yard).is_some_and(|y| (y.workers.len() as u16) < y.jobs);
                if room {
                    self.citizens[cid].work = None;
                    self.assign_job(cid, yard);
                } else {
                    self.lay_off(cid, Some(format!("{firm} brought its own crew to {site_name}. Looking for the next site.")));
                }
            }
            self.builder_sites.entry(yard).or_default().push(bid);
        }
        self.events.push(SimEvent::BuildingChanged(bid));
    }

    // ── buying a delivery ───────────────────────────────────────────────────

    /// The delivery site `bid` is ordering now: the next instalment is
    /// paid, then each line of the stage's bill is bought and sent, each
    /// load booked on the site's `due` list before its lorry leaves (a load
    /// handed over on the spot finds it counted out). The stage counts when
    /// the last of them is in (`site_received`).
    pub(super) fn buy_site_load(&mut self, bid: BuildingId) {
        let Some(at) = self.building_lot(bid).map(|l| l.center) else { return };
        if self.needs_tender(bid) {
            self.tender_site(bid);
        }
        let lines = self.site_load(bid);
        let Some(c) = self.buildings[bid].construction.as_mut() else { return };
        let q = c.deliveries.min(Construction::DELIVERIES - 1);
        if lines.is_empty() {
            // Everything for this stage is here (or on the road): an old
            // save's stage, or a bill that shrank. Count it in.
            if c.due.is_empty() {
                c.deliveries = c.deliveries.saturating_add(1);
                c.ordered = false;
            }
            return;
        }
        c.ordered = true;
        let yard = self.buildings[bid].construction.as_ref().and_then(|c| c.contract.as_ref()).and_then(|k| k.local()).filter(|y| self.is_contractor(*y));
        self.pay_instalment(bid);
        let buyer = match yard {
            Some(y) => Account::Business(y),
            None => self.site_payer(bid),
        };
        let mut imports: Vec<(Resource, f32)> = Vec::new();
        for (r, n) in lines {
            self.builders.today.material_units += n;
            let left = self.buy_site_line(bid, at, q, yard, buyer, r, n);
            if left > 0.05 {
                imports.push((r, left));
            }
        }
        // The rest from out of town, all of the stage's imports on one lorry.
        if let Some(&(lead, _)) = imports.first() {
            let cost: f32 = imports.iter().map(|(r, n)| n * r.base_price() * IMPORT_MARKUP).sum();
            let total: f32 = imports.iter().map(|(_, n)| n).sum();
            self.economy.materials_today += cost;
            self.pay_for_materials(buyer, yard, Account::Outside, cost);
            if let Some(c) = self.buildings[bid].construction.as_mut() {
                c.due.extend(imports.iter().map(|&(good, qty)| SiteLoad { good, qty, stage: q, via: lead }));
            }
            self.import(bid, lead, total, 0.0);
            self.builders.today.import_loads += 1;
        }
    }

    /// Book a load for the site's stage `q` before its lorry goes.
    fn book_site_load(&mut self, site: BuildingId, q: u8, r: Resource, got: f32) {
        if let Some(c) = self.buildings.get_mut(site).and_then(|b| b.construction.as_mut()) {
            c.due.push(SiteLoad { good: r, qty: got, stage: q, via: r });
        }
    }

    /// One line of a delivery, `want` of good `r`: from the builder's own
    /// yard, a scrap yard (steel), or the cheapest of the merchant (which
    /// stocks the material itself) and the makers of what it is made from in
    /// town (`local_from`). Returns how much is still wanted (imported).
    #[allow(clippy::too_many_arguments)]
    fn buy_site_line(&mut self, site: BuildingId, at: Vec2, q: u8, yard: Option<BuildingId>, buyer: Account, r: Resource, want: f32) -> f32 {
        // The builder's own yard stock, in its own lorry.
        if let Some(y) = yard {
            let have = self.buildings[y].business.as_ref().map_or(0.0, |z| z.stock_of(r));
            if have >= want {
                let got = self.buildings[y].business.as_mut().unwrap().take(r, want);
                self.book_site_load(site, q, r, got);
                self.queue_truck(TruckOrigin::Building(y), Cargo { from: y, to: site, resource: r, amount: got, price: 0.0 }, SITE_LORRY_COLOR);
                self.builders.today.yard_loads += 1;
                return want - got;
            }
        }
        let src = r.row().local_from;
        // [K34] Steel cut from old boxes at a scrap yard goes in first.
        if src == Some(Resource::Metal) {
            if let Some(sy) = self.scrap_steel_for(at, want) {
                let got = self.buildings[sy].business.as_mut().unwrap().take(Resource::Metal, want);
                if got > 0.0 {
                    let cost = got * Resource::Metal.base_price();
                    self.pay_for_materials(buyer, yard, Account::Business(sy), cost);
                    if let Some(z) = self.buildings[sy].business.as_mut() {
                        z.revenue_today += cost;
                        z.deals += 1;
                    }
                    self.economy.market.deals_today += 1;
                    self.book_site_load(site, q, r, got);
                    self.ship(sy, site, r, got, cost);
                    self.builders.today.scrap_loads += 1;
                    return want - got;
                }
            }
        }
        // The cheapest of the merchant (the material itself) and the makers
        // of what it is made from.
        let merchant = self.merchant_with(r, at, want);
        let maker = src.and_then(|s| self.maker_of(s, at, want, site).map(|(m, p)| (m, p, s)));
        let use_merchant = match (merchant, maker) {
            (Some((_, pm)), Some((_, pk, _))) => pm <= pk,
            (Some(_), None) => true,
            _ => false,
        };
        if let (true, Some((m, price))) = (use_merchant, merchant) {
            let got = self.buildings[m].business.as_mut().unwrap().take(r, want);
            if got > 0.0 {
                let cost = got * price;
                self.pay_for_materials(buyer, yard, Account::Business(m), cost);
                if let Some(z) = self.buildings[m].business.as_mut() {
                    z.revenue_today += cost;
                    z.deals += 1;
                }
                self.economy.market.deals_today += 1;
                self.builders.today.trade_sales += cost;
                self.builders.today.merchant_loads += 1;
                self.builders.today.merchant_units += got;
                self.book_site_load(site, q, r, got);
                // The merchant delivers in its own lorry.
                self.queue_truck(TruckOrigin::Building(m), Cargo { from: m, to: site, resource: r, amount: got, price: cost }, SITE_LORRY_COLOR);
                return want - got;
            }
        } else if let Some((seller, price, s)) = maker {
            let got = self.buildings[seller].business.as_mut().unwrap().take(s, want);
            if got > 0.0 {
                let cost = got * price;
                self.pay_for_materials(buyer, yard, Account::Business(seller), cost);
                if let Some(z) = self.buildings[seller].business.as_mut() {
                    z.revenue_today += cost;
                    z.deals += 1;
                }
                self.economy.market.deals_today += 1;
                self.builders.today.maker_loads += 1;
                self.book_site_load(site, q, r, got);
                let cargo = Cargo { from: seller, to: site, resource: r, amount: got, price: cost };
                if let Some(y) = yard.filter(|y| bld::lorries(self.contractor_crews(*y)) > 0) {
                    // The builder fetches it in its own lorry.
                    self.builders.today.collected += 1;
                    self.queue_truck(TruckOrigin::Building(y), cargo, SITE_LORRY_COLOR);
                } else {
                    // A haulier brings it.
                    self.ship(seller, site, r, got, cost);
                }
                return want - got;
            }
        }
        want
    }

    /// The nearest open merchant in reach with `want` of `r` on its racks,
    /// and its trade price.
    fn merchant_with(&self, r: Resource, at: Vec2, want: f32) -> Option<(BuildingId, f32)> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.operating() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::BuildersMerchant && !z.closed && z.stock_of(r) >= want))
            .filter_map(|(m, b)| self.lots.get(b.lot).map(|l| (m, b, l.center.distance(at))))
            .filter(|(_, _, d)| *d <= bld::LOCAL_REACH)
            .min_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)))
            .map(|(m, b, _)| (m, b.business.as_ref().map_or(0.0, |z| z.sell_price(r, b.jobs))))
    }

    /// The buyer of a site's materials pays the seller; a contractor books
    /// it as spending.
    fn pay_for_materials(&mut self, buyer: Account, yard: Option<BuildingId>, to: Account, cost: f32) {
        self.transfer(buyer, to, cost, Kind::Building);
        if let Some(z) = yard.and_then(|y| self.buildings.get_mut(y)).and_then(|b| b.business.as_mut()) {
            z.spend_today += cost;
        }
    }

    /// The instalment due as a delivery is ordered: what is left of the
    /// price over the deliveries still to come. To a contractor in town, or
    /// out of town for a firm from there (its fee: the materials it buys
    /// are paid for as they are bought).
    fn pay_instalment(&mut self, bid: BuildingId) {
        let Some(c) = self.buildings.get(bid).and_then(|b| b.construction.as_ref()) else { return };
        let Some(k) = c.contract.clone() else { return };
        let left = Construction::DELIVERIES.saturating_sub(c.deliveries).max(1) as f32;
        let payer = self.site_payer(bid);
        match k.builder {
            SiteBuilder::Local(y) => {
                let due = ((k.price - k.paid) / left).max(0.0);
                if due <= 0.0 || !self.is_contractor(y) {
                    return;
                }
                self.transfer(payer, Account::Business(y), due, Kind::Building);
                if let Some(z) = self.buildings.get_mut(y).and_then(|b| b.business.as_mut()) {
                    z.revenue_today += due;
                    z.deals += 1;
                }
                self.builders.today.contracts_paid += due;
                if let Some(k) = self.buildings.get_mut(bid).and_then(|b| b.construction.as_mut()).and_then(|c| c.contract.as_mut()) {
                    k.paid += due;
                }
            }
            SiteBuilder::OutOfTown { .. } => {
                // Its fee is its labour at the markup; materials are paid
                // for line by line.
                let (materials, _) = self.job_estimate(bid);
                let fee_total = (k.price - materials).max(0.0);
                let due = ((fee_total - k.paid) / left).max(0.0);
                if due <= 0.0 {
                    return;
                }
                self.transfer(payer, Account::Outside, due, Kind::Building);
                self.builders.today.outside_paid += due;
                if let Some(k) = self.buildings.get_mut(bid).and_then(|b| b.construction.as_mut()).and_then(|c| c.contract.as_mut()) {
                    k.paid += due;
                }
            }
        }
    }

    /// Site `bid` opened: the books, and the contractor's crew step out.
    pub(super) fn site_finished(&mut self, bid: BuildingId, site: &Construction) {
        let tick = self.clock.tick;
        let Some(k) = &site.contract else { return };
        self.builders.today.finished += 1;
        self.builders.today.build_days += tick.saturating_sub(k.since) as f32 / crate::clock::MINUTES_PER_DAY as f32;
        let Some(yard) = k.local() else { return };
        self.builders.today.finished_local += 1;
        if let Some(v) = self.builder_sites.get_mut(&yard) {
            v.retain(|s| *s != bid);
        }
        let entrance = self.building_entrance(bid).unwrap_or(Vec2::ZERO);
        let crew: Vec<CitizenId> = self.buildings.get(yard).map_or(Vec::new(), |y| y.workers.clone());
        let name = self.buildings.get(bid).map(|b| b.name.clone()).unwrap_or_default();
        for cid in crew {
            let Some(c) = self.citizens.get_mut(cid) else { continue };
            if c.inside == Some(bid) {
                c.inside = None;
                c.pos = entrance;
                c.prev_pos = entrance;
                c.activity = Activity::Idle;
                c.next_decision_tick = tick + 1;
                c.remember(tick, format!("Topped out {name}. On to the next job."));
            }
        }
    }

    // ── the hour and the day ────────────────────────────────────────────────

    /// Every sim hour: the crews' sites list; the merchants restock their
    /// trade stock; at the start of the working day the contractors fill
    /// their yards from the merchant.
    pub(super) fn builders_hourly(&mut self) {
        self.refresh_builder_sites();
        let ids: Vec<(BuildingId, BusinessKind)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.operating())
            .filter_map(|(id, b)| b.business.as_ref().filter(|z| !z.closed && matches!(z.kind, BusinessKind::Contractor | BusinessKind::BuildersMerchant)).map(|z| (id, z.kind)))
            .collect();
        if ids.is_empty() {
            return;
        }
        self.refresh_seller_index();
        let materials = crate::catalogue::site_materials();
        let morning = self.clock.hour() == 7;
        for (id, kind) in ids {
            match kind {
                BusinessKind::BuildersMerchant => self.merchant_restock(id, &materials),
                BusinessKind::Contractor if morning => self.yard_restock(id, &materials),
                _ => {}
            }
        }
    }

    /// A merchant tops up each material it trades in: from the makers in
    /// town (not another merchant), else a bulk import.
    fn merchant_restock(&mut self, id: BuildingId, materials: &[Resource]) {
        let Some(at) = self.building_lot(id).map(|l| l.center) else { return };
        let jobs = self.buildings[id].jobs.max(4) as f32;
        let target = bld::MERCHANT_STOCK_PER_JOB * jobs;
        // Its money shared over the materials it is short of, so one dear
        // line does not empty the till before the racks are all stocked.
        let short = materials
            .iter()
            .filter(|r| self.buildings[id].business.as_ref().is_some_and(|z| z.stock_of(**r) + z.inbound_of(**r) < target * bld::MERCHANT_REORDER))
            .count()
            .max(1) as f32;
        let budget = self.buildings[id].business.as_ref().map_or(0.0, |z| z.money.max(0.0)) / short;
        for &r in materials {
            let (stock, inbound, money) = {
                let z = self.buildings[id].business.as_ref().unwrap();
                (z.stock_of(r), z.inbound_of(r), z.money)
            };
            if stock + inbound >= target * bld::MERCHANT_REORDER {
                continue;
            }
            // Nothing bought on money it has not got: what it can afford of
            // the shortfall (a bulk import with its duty is the dearest way).
            let unit = r.base_price() * bld::MERCHANT_BULK_IMPORT * (1.0 + self.economy.taxes.import_duty.max(0.0));
            let want = (target - stock - inbound).min(money.max(0.0).min(budget) / unit.max(0.01)).floor();
            if want < target * 0.1 {
                continue;
            }
            // From a maker of what it is made from (`local_from`), else imported.
            let src = r.row().local_from;
            if let Some((seller, price, s)) = src.and_then(|s| self.maker_of(s, at, want, id).map(|(m, p)| (m, p, s))) {
                let got = self.buildings[seller].business.as_mut().unwrap().take(s, want);
                if got > 0.0 {
                    let cost = got * price;
                    self.transfer(Account::Business(id), Account::Business(seller), cost, Kind::Supplies);
                    if let Some(z) = self.buildings[seller].business.as_mut() {
                        z.revenue_today += cost;
                        z.deals += 1;
                    }
                    if let Some(z) = self.buildings[id].business.as_mut() {
                        z.spend_today += cost;
                    }
                    self.economy.market.deals_today += 1;
                    self.ship(seller, id, r, got, cost);
                    continue;
                }
            }
            let cost = want * r.base_price() * bld::MERCHANT_BULK_IMPORT;
            // [tax] The import duty on top, as on any business's import.
            let duty = cost * self.economy.taxes.import_duty;
            self.transfer(Account::Business(id), Account::Outside, cost, Kind::Imports);
            if duty > 0.0 {
                self.pay_city(Account::Business(id), Flow::ImportDuty, duty, None);
                self.economy.taxes.duty_today += duty;
            }
            if let Some(z) = self.buildings[id].business.as_mut() {
                z.spend_today += cost + duty;
            }
            self.economy.taxes.note_import(r, want);
            self.economy.market.imports_today += cost;
            self.import(id, r, want, cost);
        }
    }

    /// The cheapest maker of `r` in town with stock for a buyer at `at`
    /// (no merchant: they buy from the makers).
    fn maker_of(&mut self, r: Resource, at: Vec2, amount: f32, buyer: BuildingId) -> Option<(BuildingId, f32)> {
        self.refresh_seller_index();
        let mut best: Option<(BuildingId, f32)> = None;
        for &(id, pos) in self.seller_index.get(&r).map(|v| v.as_slice()).unwrap_or(&[]) {
            if id == buyer {
                continue;
            }
            let Some(b) = self.buildings.get(id) else { continue };
            let Some(z) = &b.business else { continue };
            if z.closed || !b.operating() || z.kind == BusinessKind::BuildersMerchant || z.stock_of(r) < amount * 0.5 {
                continue;
            }
            let price = z.sell_price(r, b.jobs) + HAULAGE * pos.distance(at) * r.base_price();
            if best.is_none_or(|(_, p)| price < p) {
                best = Some((id, price));
            }
        }
        best
    }

    /// A contractor's morning run to the merchant: its yard filled to a
    /// load per crew of each material, from the nearest merchant with stock.
    fn yard_restock(&mut self, id: BuildingId, materials: &[Resource]) {
        let crews = self.contractor_crews(id);
        if crews == 0 {
            return;
        }
        let Some(at) = self.building_lot(id).map(|l| l.center) else { return };
        let target = bld::YARD_UNITS_PER_CREW * crews as f32;
        for &r in materials {
            let (stock, inbound, money) = {
                let z = self.buildings[id].business.as_ref().unwrap();
                (z.stock_of(r), z.inbound_of(r), z.money)
            };
            if stock + inbound >= target * 0.5 {
                continue;
            }
            let want = target - stock - inbound;
            let merchant = self
                .buildings
                .iter()
                .filter(|(_, b)| b.operating() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::BuildersMerchant && !z.closed && z.stock_of(r) >= want * 0.5))
                .filter_map(|(m, b)| self.lots.get(b.lot).map(|l| (m, l.center.distance(at))))
                .filter(|(_, d)| *d <= bld::LOCAL_REACH)
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
                .map(|(m, _)| m);
            let Some(m) = merchant else { continue };
            let jobs = self.buildings[m].jobs;
            let price = self.buildings[m].business.as_ref().map_or(0.0, |z| z.sell_price(r, jobs));
            // What it can pay for, of what the racks hold.
            let have = self.buildings[m].business.as_ref().map_or(0.0, |z| z.stock_of(r));
            let want = want.min(have).min(money.max(0.0) / price.max(0.01)).floor();
            if want < target * 0.25 {
                continue;
            }
            let got = self.buildings[m].business.as_mut().unwrap().take(r, want);
            let cost = got * price;
            self.transfer(Account::Business(id), Account::Business(m), cost, Kind::Supplies);
            if let Some(z) = self.buildings[m].business.as_mut() {
                z.revenue_today += cost;
                z.deals += 1;
            }
            if let Some(z) = self.buildings[id].business.as_mut() {
                z.spend_today += cost;
            }
            self.builders.today.trade_sales += cost;
            self.builders.today.merchant_units += got;
            self.economy.market.deals_today += 1;
            // Fetched in its own lorry.
            self.note_inbound(id, r, got);
            self.queue_truck(TruckOrigin::Building(id), Cargo { from: m, to: id, resource: r, amount: got, price: cost }, SITE_LORRY_COLOR);
        }
    }

    /// Midnight, before the businesses close their books: the merchants'
    /// takings and the contractors' standing go into the day's books, which
    /// roll. (Contractors going bust are counted as they shut,
    /// `note_business_closed`.)
    pub(super) fn builders_daily(&mut self) {
        let mut t = self.builders.today;
        t.contractors = 0;
        t.contractor_money = 0.0;
        t.crews = 0;
        t.merchants = 0;
        t.merchant_takings = 0.0;
        for b in self.buildings.values() {
            let Some(z) = &b.business else { continue };
            if z.closed || !b.operating() {
                continue;
            }
            match z.kind {
                BusinessKind::Contractor => {
                    t.contractors += 1;
                    t.contractor_money += z.money;
                    t.crews += bld::crews(b.workers.len()) as u32;
                }
                BusinessKind::BuildersMerchant => {
                    t.merchants += 1;
                    t.merchant_takings += z.revenue_today;
                }
                _ => {}
            }
        }
        self.builders.today = t;
        self.builders.roll(self.clock.day() as u32);
    }

    /// A business shut for good today: a contractor going bust gives its
    /// jobs back to tender at their next order.
    pub(super) fn note_business_closed(&mut self, kind: BusinessKind) {
        if kind == BusinessKind::Contractor {
            self.builders.today.busts += 1;
        }
    }

    /// Today's books so far.
    pub fn builders_today(&self) -> &BuildersDay {
        &self.builders.today
    }
}
