//! [megacorp] Mega-corporations on the `World`.
//!
//! *Growing into one* (`promote_companies`, daily, a [`PROMOTE_CHANCE`]
//! roll): a company is an owner record of a firm or a founder, or a lone
//! business. Its size is its holdings, their staff and their takings (the
//! last three days' average). The biggest one past the thresholds, in a town
//! of [`CORP_MIN_POP`] people with room for another corp, becomes a
//! mega-corp: a coined name, a brand colour and a logo (`crate::corp`).
//!
//! *Its day* (`daily_corps`): it skims [`SKIM`] of each holding's profit into
//! its purse (never below [`SKIM_FLOOR`] in the till), pays the player's
//! levy, builds its headquarters downtown if it has none (a tower on the
//! dearest commercial plots it can assemble, `build_hq`), buys what is
//! nearest its HQ by road (`corp_buy`: businesses in profit of every kind,
//! a new kind counting as nearer, homes for its staff while it has fewer
//! than [`HOUSING_PER_STAFF`] a head, and vacant land of any zoning to open
//! a branded business on), moves staff into its housing (rent
//! [`STAFF_RENT_DISCOUNT`] off), and sells down to the cap when the player
//! lowers it. Road distances come from one Dijkstra per HQ, cached against
//! the road network's version.
//!
//! Draws come from the estate's hash-seeded generators (`erng`) and hashes,
//! never the world's own stream.

use super::*;
use crate::corp::*;
use crate::estate::{lots_touch, merged_lot, OwnerId, OwnerKind, MAX_MERGED_LOTS};
use slotmap::Key;
use crate::vehicle::Livery;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A company's size, for promotion and the Corporations page.
#[derive(Clone, Debug, PartialEq)]
pub struct CompanySize {
    /// The owner record, or `None` for a lone business.
    pub owner: Option<OwnerId>,
    /// Its biggest business.
    pub flagship: BuildingId,
    pub name: String,
    pub kind: BusinessKind,
    pub holdings: usize,
    pub staff: usize,
    /// Average daily takings over the last three days, summed.
    pub revenue: f32,
}

impl CompanySize {
    pub fn qualifies(&self) -> bool {
        (self.revenue >= PROMOTE_REVENUE && self.staff >= PROMOTE_STAFF && self.holdings >= PROMOTE_HOLDINGS) || (self.revenue >= PROMOTE_REVENUE * 2.0 && self.staff >= PROMOTE_STAFF * 2)
    }
}

/// Something a corp could buy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Buy {
    /// A building standing (a business or a block of homes).
    Building(BuildingId),
    /// A vacant lot to open a branded business on.
    Land(LotId),
}

/// Ordered f32 for the Dijkstra heap.
#[derive(PartialEq, PartialOrd)]
struct Dist(f32, u32);
impl Eq for Dist {}
impl Ord for Dist {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0).then(self.1.cmp(&o.1))
    }
}

impl World {
    // ── lookups ────────────────────────────────────────────────────────────

    /// The mega-corp owning `bid`, if any.
    pub fn corp_of(&self, bid: BuildingId) -> Option<&MegaCorp> {
        let o = self.buildings.get(bid)?.plot.owner?;
        self.corps.corp_of_owner(o)
    }

    /// Index of the corp with owner `o` in `corps.corps`.
    pub(super) fn corp_index(&self, o: OwnerId) -> Option<usize> {
        self.corps.corps.iter().position(|c| c.owner == o)
    }

    /// The university whose campus `bid` is part of (main building, annex
    /// or a dorm it bought), if any.
    pub fn campus_of(&self, bid: BuildingId) -> Option<&Campus> {
        if let Some(c) = self.corps.campus_of_building(bid) {
            return Some(c);
        }
        let o = self.buildings.get(bid)?.plot.owner?;
        self.corps.campus_of_owner(o)
    }

    /// The brand shown on `bid` and in what role, for the renderer: a
    /// corp's HQ, holding or staff housing, a university or its annex.
    pub fn brand_mark(&self, bid: BuildingId) -> Option<BrandMark> {
        let b = self.buildings.get(bid)?;
        if let Some(c) = self.corp_of(bid) {
            let role = if c.hq == Some(bid) {
                BrandRole::Headquarters
            } else if b.business.is_none() && b.homes > 0 {
                BrandRole::Housing
            } else {
                BrandRole::Holding
            };
            return Some(c.brand.mark(role));
        }
        let k = self.campus_of(bid)?;
        let role = if k.building == bid { BrandRole::Campus } else { BrandRole::Annex(k.annex_of(bid).or(b.annex).unwrap_or(Annex::Dorm)) };
        Some(k.brand.mark(role))
    }

    /// The fleet livery of a corp-owned business's vehicles.
    pub fn corp_livery(&self, bid: BuildingId) -> Option<Livery> {
        let c = self.corp_of(bid)?;
        let letter = (c.brand.monogram.to_ascii_uppercase() as u8).saturating_sub(b'A').min(25);
        Some(Livery::Corp(c.brand.colour, c.brand.shape as u8 | (letter << 3)))
    }

    /// Buildings the market trades in: not the city's, not parks.
    pub fn ordinary_buildings(&self) -> usize {
        self.buildings.values().filter(|b| !b.is_special() && !b.is_parking() && b.land_use != LandUse::Park).count()
    }

    /// Most properties one corp may hold under the player's rule.
    pub fn corp_cap(&self) -> usize {
        ((self.ordinary_buildings() as f32 * self.corps.rules.max_share.clamp(MIN_MAX_SHARE, MAX_MAX_SHARE)) as usize).max(MIN_CAP)
    }

    /// How many corps the town carries now.
    pub fn corp_room(&self) -> usize {
        if self.citizens.len() < CORP_MIN_POP {
            return 0;
        }
        (1 + self.citizens.len() / CORP_PEOPLE_EACH).min(CORP_MAX)
    }

    /// Everyone working at a building the corp owns.
    pub fn corp_staff(&self, owner: OwnerId) -> Vec<CitizenId> {
        let Some(o) = self.estate.owners.get(owner) else { return Vec::new() };
        let mut v: Vec<CitizenId> = o.properties.iter().filter_map(|p| self.buildings.get(*p)).filter(|b| b.operating()).flat_map(|b| b.workers.iter().copied()).collect();
        v.sort();
        v.dedup();
        v
    }

    /// Homes in the corp's staff housing (residential holdings).
    pub fn corp_homes(&self, owner: OwnerId) -> (u32, Vec<BuildingId>) {
        let Some(o) = self.estate.owners.get(owner) else { return (0, Vec::new()) };
        let blocks: Vec<BuildingId> = o.properties.iter().copied().filter(|p| self.buildings.get(*p).is_some_and(|b| b.land_use == LandUse::Residential && b.business.is_none())).collect();
        (blocks.iter().map(|p| self.buildings[*p].planned_capacity().0 as u32).sum(), blocks)
    }

    /// Share of the town's business takings (yesterday) that went to the
    /// corp's holdings.
    pub fn corp_market_share(&self, owner: OwnerId) -> f32 {
        let total: f32 = self.buildings.values().filter_map(|b| b.business.as_ref()).filter_map(|z| z.books.last()).map(|d| d.revenue.max(0.0)).sum();
        if total <= 0.0 {
            return 0.0;
        }
        let Some(o) = self.estate.owners.get(owner) else { return 0.0 };
        let mine: f32 = o.properties.iter().filter_map(|p| self.buildings.get(*p)).filter_map(|b| b.business.as_ref()).filter_map(|z| z.books.last()).map(|d| d.revenue.max(0.0)).sum();
        mine / total
    }

    /// The rent a household pays for `home` a day: the building's rent,
    /// less the staff discount in its employer's housing and the dorm
    /// discount for a student in their university's dorm.
    pub fn rent_due(&self, hid: HouseholdId, home: BuildingId) -> f32 {
        let Some(b) = self.buildings.get(home) else { return 0.0 };
        let rent = b.rent;
        let Some(owner) = b.plot.owner else { return rent };
        let Some(h) = self.households.get(hid) else { return rent };
        if self.corps.corp_of_owner(owner).is_some() {
            let staff = h.members.iter().any(|m| self.citizens.get(*m).and_then(|c| c.work).and_then(|w| self.buildings.get(w)).is_some_and(|w| w.plot.owner == Some(owner)));
            if staff {
                return rent * (1.0 - STAFF_RENT_DISCOUNT);
            }
        } else if let Some(k) = self.corps.campus_of_owner(owner) {
            if h.members.iter().any(|m| k.students.contains(m)) {
                return rent * (1.0 - DORM_RENT_DISCOUNT);
            }
        }
        rent
    }

    // ── road distance from the HQ ──────────────────────────────────────────

    /// Where a corp measures its buys from: the HQ once it stands (or is
    /// going up), else its flagship.
    pub(super) fn corp_origin(&self, ci: usize) -> Option<BuildingId> {
        let c = self.corps.corps.get(ci)?;
        c.hq.filter(|b| self.buildings.contains_key(*b)).or(c.flagship.filter(|b| self.buildings.contains_key(*b))).or_else(|| self.estate.owners.get(c.owner)?.properties.iter().copied().find(|b| self.buildings.contains_key(*b)))
    }

    /// Road distances (metres) from `origin`'s street access to every road
    /// node, cached against the road network's version.
    pub(super) fn drive_table(&mut self, origin: BuildingId) -> Option<&Vec<f32>> {
        let version = self.roads.version;
        let fresh = self.corps.drive_cache.get(&origin).is_some_and(|(v, d)| *v == version && d.len() == self.roads.nodes.len());
        if !fresh {
            let lot = self.building_lot(origin)?.clone();
            let (seg, p) = self.lot_access(&lot)?;
            let n = self.roads.nodes.len();
            let mut best = vec![f32::INFINITY; n];
            let mut heap = BinaryHeap::new();
            let s = self.roads.segment(seg);
            for node in [s.a, s.b] {
                let d = p.distance(self.roads.node(node).pos);
                if d < best[node.0 as usize] {
                    best[node.0 as usize] = d;
                    heap.push(Reverse(Dist(d, node.0)));
                }
            }
            while let Some(Reverse(Dist(d, node))) = heap.pop() {
                if d > best[node as usize] {
                    continue;
                }
                let here = self.roads.node(NodeId(node)).pos;
                for (nb, sid) in self.roads.neighbors(NodeId(node)) {
                    if self.roads.segment(sid).is_rail() {
                        continue;
                    }
                    let nd = d + here.distance(self.roads.node(nb).pos);
                    if nd < best[nb.0 as usize] {
                        best[nb.0 as usize] = nd;
                        heap.push(Reverse(Dist(nd, nb.0)));
                    }
                }
            }
            self.corps.drive_cache.insert(origin, (version, best));
        }
        self.corps.drive_cache.get(&origin).map(|(_, d)| d)
    }

    /// Metres by road from the cached table to `lot`; infinite off the
    /// network or out of reach.
    pub(super) fn drive_to_lot(&self, table: &[f32], origin_seg: Option<(SegmentId, Vec2)>, lot: &Lot) -> f32 {
        let Some((seg, p)) = self.lot_access(lot) else { return f32::INFINITY };
        let s = self.roads.segment(seg);
        let mut d = [s.a, s.b].iter().map(|nd| table.get(nd.0 as usize).copied().unwrap_or(f32::INFINITY) + p.distance(self.roads.node(*nd).pos)).fold(f32::INFINITY, f32::min);
        if let Some((os, op)) = origin_seg {
            if os == seg {
                d = d.min(p.distance(op));
            }
        }
        d
    }

    /// Road distance from the corp's HQ (or flagship) to `lot`, metres.
    pub fn corp_drive_m(&mut self, owner: OwnerId, lot: LotId) -> Option<f32> {
        let ci = self.corp_index(owner)?;
        let origin = self.corp_origin(ci)?;
        let table = self.drive_table(origin)?.clone();
        let oseg = self.building_lot(origin).and_then(|l| self.lot_access(l));
        let l = self.lots.get(lot)?;
        Some(self.drive_to_lot(&table, oseg, l))
    }

    // ── growing into one ───────────────────────────────────────────────────

    /// Every company in town with its size, biggest takings first: owner
    /// records of firms and founders, and lone businesses.
    pub fn companies(&self) -> Vec<CompanySize> {
        let avg = |b: &Building| -> f32 {
            b.business.as_ref().map_or(0.0, |z| {
                let n = z.books.len().min(3);
                if n == 0 {
                    0.0
                } else {
                    z.books[z.books.len() - n..].iter().map(|d| d.revenue).sum::<f32>() / n as f32
                }
            })
        };
        let eligible = |b: &Building| b.business.as_ref().is_some_and(|z| !z.closed && !matches!(z.kind, BusinessKind::CargoHub | BusinessKind::Parking)) && b.operating() && !b.is_special();
        let mut out: Vec<CompanySize> = Vec::new();
        for (oid, o) in self.estate.owners.iter() {
            if !matches!(o.kind, OwnerKind::Firm { .. } | OwnerKind::Person { .. }) {
                continue;
            }
            let held: Vec<(BuildingId, &Building)> = o.properties.iter().filter_map(|p| self.buildings.get(*p).map(|b| (*p, b))).filter(|(_, b)| eligible(b)).collect();
            if held.is_empty() {
                continue;
            }
            let (flagship, fb) = held.iter().max_by(|a, b| a.1.workers.len().cmp(&b.1.workers.len()).then(b.0.cmp(&a.0))).copied().unwrap();
            out.push(CompanySize {
                owner: Some(oid),
                flagship,
                name: o.name.clone(),
                kind: fb.business.as_ref().unwrap().kind,
                holdings: o.properties.len(),
                staff: held.iter().map(|(_, b)| b.workers.len()).sum(),
                revenue: held.iter().map(|(_, b)| avg(b)).sum(),
            });
        }
        for (id, b) in self.buildings.iter() {
            if b.plot.owner.is_some() || !eligible(b) {
                continue;
            }
            out.push(CompanySize { owner: None, flagship: id, name: b.name.clone(), kind: b.business.as_ref().unwrap().kind, holdings: 1, staff: b.workers.len(), revenue: avg(b) });
        }
        out.sort_by(|a, b| b.revenue.total_cmp(&a.revenue).then(a.flagship.cmp(&b.flagship)));
        out
    }

    /// Once a day: maybe one company grows into a mega-corp.
    pub(super) fn promote_companies(&mut self) {
        if !self.corps.rules.allowed || self.corps.corps.len() >= self.corp_room() {
            return;
        }
        let mut rng = self.erng(0xC0_4B);
        if !rng.random_bool(PROMOTE_CHANCE) {
            return;
        }
        let Some(best) = self.companies().into_iter().find(|c| c.qualifies()) else { return };
        let _ = self.promote_to_corp(best.flagship);
    }

    /// The company owning `bid` becomes a mega-corp (the player's
    /// fast-forward, and the daily promotion). Returns its index.
    pub fn promote_to_corp(&mut self, bid: BuildingId) -> Result<usize, String> {
        let b = self.buildings.get(bid).ok_or("no such building")?;
        let kind = b.business.as_ref().map(|z| z.kind).ok_or("not a business")?;
        if b.is_special() {
            return Err("the city's own buildings are not companies".into());
        }
        if let Some(o) = b.plot.owner {
            if let Some(i) = self.corp_index(o) {
                return Ok(i);
            }
            if let Some(ow) = self.estate.owners.get(o) {
                if !matches!(ow.kind, OwnerKind::Firm { .. } | OwnerKind::Person { .. } | OwnerKind::Developer) {
                    return Err(format!("{} is owned by {}, not a company", b.name, ow.name));
                }
            }
        }
        let origin_name = self.owner_of(bid).map_or(b.name.clone(), |(_, o)| o.name.clone());
        let pos = self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.center);
        // A name nobody has, from the seed, the building and the day.
        let h = mix(self.config.seed ^ 0xC0_4B_C0, bid.data().as_ffi() ^ (self.clock.day() << 32));
        let taken: Vec<String> = self.corps.corps.iter().map(|c| c.brand.name.clone()).chain(self.estate.owners.values().map(|o| o.name.clone())).collect();
        let name = (0..16u64).map(|k| corp_name(mix(h, k))).find(|n| !taken.contains(n)).unwrap_or_else(|| corp_name(h));
        let brand = {
            let others: Vec<&Brand> = self.corps.brands();
            new_brand(h, name.clone(), &LogoShape::CORP, &others)
        };
        let owner = match b.plot.owner.filter(|o| self.estate.owners.contains_key(*o)) {
            Some(o) => {
                let ow = &mut self.estate.owners[o];
                ow.kind = OwnerKind::MegaCorp { kind };
                ow.name = name.clone();
                o
            }
            None => {
                let o = self.new_owner(name.clone(), OwnerKind::MegaCorp { kind });
                self.give_to(bid, o, 0.0);
                o
            }
        };
        // Its businesses answer to the corp now, not to a founder's books.
        for p in self.estate.owners[owner].properties.clone() {
            if let Some(z) = self.buildings.get_mut(p).and_then(|b| b.business.as_mut()) {
                z.owner = None;
            }
            self.events.push(SimEvent::BuildingChanged(p));
        }
        let day = self.clock.day() as u32;
        self.corps.corps.push(MegaCorp { owner, brand, origin: kind, origin_name: origin_name.clone(), flagship: Some(bid), hq: None, cash: 2_000.0, founded_day: day, bought_total: 0, housed_total: 0, built_total: 0, levy_total: 0.0, revenue_yesterday: 0.0 });
        self.corps.stats.promotions_total += 1;
        let tick = self.clock.tick;
        self.social.push_news(tick, format!("🏢 {origin_name} is now {name}, a mega-corp. Its tower is coming downtown."), pos, None);
        let ci = self.corps.corps.len() - 1;
        self.build_hq(ci);
        Ok(ci)
    }

    // ── the day ────────────────────────────────────────────────────────────

    /// Once a day, after the land market: promotions, each corp's books,
    /// HQ, buys, staff housing and the cap; then the campuses.
    pub(super) fn daily_corps(&mut self) {
        crate::prof_scope!("daily_corps");
        self.corps.stats.purchases_today = 0;
        self.corps.stats.levy_today = 0.0;
        self.corps.stats.housed_today = 0;
        self.prune_corps();
        self.promote_companies();
        for ci in 0..self.corps.corps.len() {
            self.corp_books(ci);
            let hq_ok = self.corps.corps[ci].hq.is_some_and(|b| self.buildings.contains_key(b));
            if !hq_ok {
                self.corps.corps[ci].hq = None;
                self.build_hq(ci);
            }
            self.divest(ci);
            let mut rng = self.erng(0xC0_B0 + ci as u64);
            let held = self.estate.owners.get(self.corps.corps[ci].owner).map_or(0, |o| o.properties.len());
            let buys = (1 + held / 8).min(BUYS_PER_DAY);
            for _ in 0..buys {
                if rng.random_bool(0.6) && !self.corp_buy(ci) {
                    break;
                }
            }
            self.house_staff(ci);
        }
        self.daily_campuses();
    }

    /// Corps whose owner record is gone (every property sold or pulled
    /// down) are wound up; their HQ and flagship pointers are cleared when
    /// those buildings went.
    pub(super) fn prune_corps(&mut self) {
        let owners = &self.estate.owners;
        let buildings = &self.buildings;
        self.corps.corps.retain(|c| owners.get(c.owner).is_some_and(|o| !o.properties.is_empty()));
        for c in &mut self.corps.corps {
            if c.hq.is_some_and(|b| !buildings.contains_key(b)) {
                c.hq = None;
            }
            if c.flagship.is_some_and(|b| !buildings.contains_key(b)) {
                c.flagship = None;
            }
        }
        let keep: Vec<BuildingId> = self.corps.corps.iter().filter_map(|c| c.hq.or(c.flagship)).collect();
        self.corps.drive_cache.retain(|b, _| keep.contains(b));
    }

    /// Skim each holding's profit into the corp's purse and pay the levy.
    pub(super) fn corp_books(&mut self, ci: usize) {
        let owner = self.corps.corps[ci].owner;
        let props = self.estate.owners.get(owner).map(|o| o.properties.clone()).unwrap_or_default();
        let (mut skim, mut gross) = (0.0f64, 0.0f32);
        for p in props {
            let Some(z) = self.buildings.get_mut(p).and_then(|b| b.business.as_mut()) else { continue };
            let Some(d) = z.books.last().copied() else { continue };
            gross += d.revenue.max(0.0);
            let profit = d.revenue - d.spend;
            if profit > 0.0 {
                let take = (profit * SKIM).min((z.money - SKIM_FLOOR).max(0.0));
                z.money -= take;
                skim += take as f64;
            }
        }
        let levy = (gross * self.corps.rules.levy.clamp(0.0, MAX_LEVY)) as f64;
        let c = &mut self.corps.corps[ci];
        c.revenue_yesterday = gross;
        c.cash += skim;
        if levy > 0.0 {
            // [review] `cash` already holds the skim: adding it again let the
            // levy drive the corp into debt.
            let paid = levy.min(c.cash.max(0.0));
            c.cash -= paid;
            c.levy_total += paid;
            let name = c.brand.name.clone();
            self.economy.credit_item(Flow::BusinessTax, paid as f32, Item::Named(format!("mega-corp levy: {name}")));
            self.corps.stats.levy_today += paid;
            self.corps.stats.levy_total += paid;
        }
    }

    // ── headquarters ───────────────────────────────────────────────────────

    /// [supertall] Where the city allows one (1,200 people, a downtown
    /// tower site, a height limit over 150 m there), the HQ is a league
    /// tower: a supertall (320 m) if the limit lets it, else as tall a
    /// skyscraper as it does. Built by a tower developer on the corp's
    /// order and handed to the corp. `None` when the city does not allow it.
    pub(super) fn hq_league_tower(&mut self, ci: usize) -> Option<BuildingId> {
        use crate::towers::{SKYSCRAPER_M, SUPERTALL_M, TOWER_MAX_M, TOWER_MIN_POP};
        if self.citizens.len() < TOWER_MIN_POP {
            return None;
        }
        let site = self.tower_sites(false).into_iter().find(|t| self.lots.get(t.lot).is_some_and(|l| l.land_use == LandUse::Commercial))?;
        let limit = site.limit.unwrap_or(TOWER_MAX_M);
        let height = if limit >= SUPERTALL_M + 20.0 {
            SUPERTALL_M + 20.0
        } else if limit >= SKYSCRAPER_M {
            limit.min(240.0)
        } else {
            return None;
        };
        let near = self.lots.get(site.lot).map(|l| l.center);
        let bid = self.start_tower(super::TowerOrder { height: Some(height), design: None, near, force: true }).ok()?;
        let (owner, name) = (self.corps.corps[ci].owner, self.corps.corps[ci].brand.name.clone());
        let short = name.split_whitespace().next().unwrap_or(&name).to_string();
        self.give_to(bid, owner, 0.0);
        let jobs = self.buildings[bid].planned_capacity().1;
        let b = &mut self.buildings[bid];
        b.name = format!("{short} Tower");
        if b.land_use == LandUse::Commercial {
            b.business = Some(Business::new(BusinessKind::Office, jobs));
        }
        self.corps.corps[ci].hq = Some(bid);
        let tick = self.clock.tick;
        let pos = self.lots.get(self.buildings[bid].lot).map_or(Vec2::ZERO, |l| l.center);
        self.social.push_news(tick, format!("🏗 {name} broke ground on {short} Tower, a {height:.0} m headquarters."), pos, None);
        self.events.push(SimEvent::BuildingChanged(bid));
        Some(bid)
    }

    /// Floors for an HQ on `lot` when it is not a league tower: tall, above
    /// what the pressure would build, within the pressure ceiling.
    pub fn hq_floors(&self, lot: LotId) -> u8 {
        let base = self.height_cap(lot).max(self.pressure_floors(lot));
        base.saturating_add(10).clamp(HQ_MIN_FLOORS, HQ_MAX_FLOORS)
    }

    /// May an HQ site take this plot? Commercial, vacant or a grown
    /// building the market may buy that is not another corp's, no form or
    /// merge of its own, not promised or frozen.
    pub(super) fn hq_plot_ok(&self, lid: LotId) -> bool {
        let Some(l) = self.lots.get(lid) else { return false };
        if l.land_use != LandUse::Commercial || !self.site_clear(lid, SiteRule::STREET_NOT_HISTORIC) {
            return false;
        }
        match l.building {
            None => true,
            Some(b) => self.buyable(b) && self.buildings.get(b).is_some_and(|x| !x.plot.merged() && x.plot.form.is_none() && x.floors <= 14 && x.plot.owner.is_none_or(|o| self.corps.corp_of_owner(o).is_none() && self.corps.campus_of_owner(o).is_none())),
        }
    }

    /// The best downtown site: the dearest, most pressed commercial plot
    /// with street access, with touching plots added (up to
    /// [`HQ_MAX_PLOTS`]) while the footprint is under 900 m².
    pub fn hq_site(&self) -> Option<Vec<LotId>> {
        let mut cands: Vec<(f32, LotId)> = self
            .lots
            .iter()
            .filter(|(id, l)| l.car_access() && !l.landlocked && self.hq_plot_ok(*id))
            .map(|(id, l)| (self.lot_value(id) * (1.0 + self.pressure_floors(id) as f32 / 10.0) + l.area() / 20_000.0, id))
            .collect();
        cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        for (_, primary) in cands.into_iter().take(24) {
            let p = &self.lots[primary];
            let mut ours = vec![primary];
            let mut area = p.area();
            while area < 900.0 && ours.len() < HQ_MAX_PLOTS.min(MAX_MERGED_LOTS) {
                let mine: Vec<&Lot> = ours.iter().filter_map(|l| self.lots.get(*l)).collect();
                let mut next: Option<(f32, LotId)> = None;
                for (lid, l) in self.lots.iter() {
                    if ours.contains(&lid) || !mine.iter().any(|m| m.center.distance(l.center) < 120.0 && lots_touch(m, l)) || !self.hq_plot_ok(lid) {
                        continue;
                    }
                    let others: Vec<&Lot> = ours.iter().skip(1).chain(std::iter::once(&lid)).filter_map(|x| self.lots.get(*x)).collect();
                    if merged_lot(p, &others).is_none() {
                        continue;
                    }
                    let cost = l.building.map_or(0.0, |b| self.sale_price(b) as f32);
                    if next.is_none_or(|(c, n)| cost < c || (cost == c && lid < n)) {
                        next = Some((cost, lid));
                    }
                }
                let Some((_, lid)) = next else { break };
                area += self.lots[lid].area();
                ours.push(lid);
            }
            if area >= 450.0 {
                return Some(ours);
            }
        }
        None
    }

    /// Build the corp's headquarters downtown: buy the site, pull down what
    /// stands, raise an office tower in its name. The HQ's building.
    pub(super) fn build_hq(&mut self, ci: usize) -> Option<BuildingId> {
        if let Some(t) = self.hq_league_tower(ci) {
            return Some(t);
        }
        let plots = self.hq_site()?;
        let (owner, name) = {
            let c = &self.corps.corps[ci];
            (c.owner, c.brand.name.clone())
        };
        let short = name.split_whitespace().next().unwrap_or(&name).to_string();
        let primary = plots[0];
        let floors = self.hq_floors(primary);
        let value = self.lot_value(primary);
        let price: f64 = plots.iter().filter_map(|l| self.lots.get(*l).and_then(|x| x.building)).map(|b| self.sale_price(b)).sum();
        let memory = format!("{name} bought the block for its headquarters. We had to go.");
        let bid = match self.lots[primary].building {
            Some(b) => {
                self.pack_business(b, &format!("{name}'s headquarters"));
                self.settle_sale(b);
                self.buildings[b].land_use = LandUse::Commercial;
                self.redevelop_with(b, Tier::High, floors, value, memory.clone(), true);
                b
            }
            None => {
                let seed = self.fresh_seed();
                let mut b = Building::new(primary, LandUse::Commercial, floors, Tier::High, seed, String::new(), self.clock.tick, value);
                b.construction = Some(Construction::new(Construction::days_for(floors, Tier::High)));
                b.recompute_capacity();
                b.update_rent(value);
                let id = self.buildings.insert(b);
                self.lots[primary].building = Some(id);
                self.events.push(SimEvent::BuildingBuilt(id));
                self.order_materials(id);
                id
            }
        };
        self.take_plots(bid, &plots[1..], &memory);
        let jobs = self.buildings[bid].planned_capacity().1;
        let b = &mut self.buildings[bid];
        b.name = format!("{short} Tower");
        b.business = Some(Business::new(BusinessKind::Office, jobs));
        self.give_to(bid, owner, price);
        let c = &mut self.corps.corps[ci];
        c.hq = Some(bid);
        c.cash -= (price * DEPOSIT_SHARE).min(c.cash.max(0.0));
        let tick = self.clock.tick;
        let pos = self.lots.get(primary).map_or(Vec2::ZERO, |l| l.center);
        self.social.push_news(tick, format!("🏗 {name} broke ground on {short} Tower, {floors} floors, its headquarters."), pos, None);
        self.events.push(SimEvent::BuildingChanged(bid));
        Some(bid)
    }

    // ── buying ─────────────────────────────────────────────────────────────

    /// What the corp may buy, each with its road distance from the HQ
    /// weighted by what it is, nearest first.
    pub(super) fn corp_candidates(&mut self, ci: usize) -> Vec<(f32, Buy)> {
        let Some(origin) = self.corp_origin(ci) else { return Vec::new() };
        let Some(table) = self.drive_table(origin).cloned() else { return Vec::new() };
        let oseg = self.building_lot(origin).and_then(|l| self.lot_access(l));
        let owner = self.corps.corps[ci].owner;
        let props = self.estate.owners.get(owner).map(|o| o.properties.clone()).unwrap_or_default();
        let kinds: Vec<BusinessKind> = props.iter().filter_map(|p| self.buildings.get(*p)?.business.as_ref().map(|z| z.kind)).collect();
        let staff = self.corp_staff(owner).len() as f32;
        let (homes, _) = self.corp_homes(owner);
        let wants_homes = (homes as f32) < staff * HOUSING_PER_STAFF;
        let mut out: Vec<(f32, Buy)> = Vec::new();
        for (bid, b) in self.buildings.iter() {
            if b.plot.owner.is_some() || b.plot.merged() || !self.buyable(bid) {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let weight = match &b.business {
                Some(z) => {
                    // A going concern: two days open at least, in profit over
                    // its last three.
                    if z.closed || z.books.len() < 2 || matches!(z.kind, BusinessKind::Parking | BusinessKind::TaxiCompany | BusinessKind::CargoHub) {
                        continue;
                    }
                    let last = &z.books[z.books.len().saturating_sub(3)..];
                    if last.iter().map(|d| d.revenue - d.spend).sum::<f32>() <= 0.0 {
                        continue;
                    }
                    if kinds.contains(&z.kind) {
                        1.0
                    } else {
                        NEW_KIND_PULL
                    }
                }
                None if b.land_use == LandUse::Residential && wants_homes && b.homes >= 4 && b.homes <= 60 && b.tier <= Tier::Mid => 0.9,
                None => continue,
            };
            let d = self.drive_to_lot(&table, oseg, lot);
            if d.is_finite() && d <= MAX_DRIVE_M {
                out.push((d * weight, Buy::Building(bid)));
            }
        }
        // Vacant land of any zoning: a branded business opens on it. It
        // counts as further off than a going concern, and at most one buy
        // in four is land (the corp buys up the town, it does not fill it).
        let c = &self.corps.corps[ci];
        let land_ok = c.built_total * 4 <= c.bought_total;
        for (lid, l) in self.lots.iter() {
            if !land_ok {
                break;
            }
            if l.land_use == LandUse::Park || !self.lot_free_for(lid, SiteRule::STREET_NOT_HISTORIC) {
                continue;
            }
            if l.land_use == LandUse::Residential && !wants_homes {
                continue;
            }
            let d = self.drive_to_lot(&table, oseg, l);
            if d.is_finite() && d <= MAX_DRIVE_M {
                out.push((d * 1.6 + 250.0, Buy::Land(lid)));
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| buy_key(&a.1).cmp(&buy_key(&b.1))));
        out
    }

    /// The corp buys the nearest thing it wants. False if it bought nothing
    /// (at its cap, out of cash, or nothing within reach).
    pub(super) fn corp_buy(&mut self, ci: usize) -> bool {
        let owner = self.corps.corps[ci].owner;
        let held = self.estate.owners.get(owner).map_or(0, |o| o.properties.len());
        if held >= self.corp_cap() || self.corps.corps[ci].cash < BUY_MIN_CASH {
            return false;
        }
        let Some((_, pick)) = self.corp_candidates(ci).into_iter().next() else { return false };
        let name = self.corps.corps[ci].brand.name.clone();
        let tick = self.clock.tick;
        let (bid, price) = match pick {
            Buy::Building(bid) => {
                let price = self.sale_price(bid);
                self.give_to(bid, owner, price);
                if let Some(z) = self.buildings.get_mut(bid).and_then(|b| b.business.as_mut()) {
                    z.owner = None;
                }
                let (bname, staff) = {
                    let b = &self.buildings[bid];
                    (b.name.clone(), b.workers.clone())
                };
                for s in staff {
                    if let Some(c) = self.citizens.get_mut(s) {
                        c.remember(tick, format!("{name} bought {bname}. We work for the corp now."));
                    }
                }
                (bid, price)
            }
            Buy::Land(lid) => {
                let Some(bid) = self.corp_build_on(lid, ci) else { return false };
                self.corps.corps[ci].built_total += 1;
                (bid, 0.0)
            }
        };
        let c = &mut self.corps.corps[ci];
        c.cash -= (price * DEPOSIT_SHARE).max(100.0).min(c.cash.max(0.0));
        c.bought_total += 1;
        self.corps.stats.purchases_total += 1;
        self.corps.stats.purchases_today += 1;
        self.events.push(SimEvent::BuildingChanged(bid));
        // The first few, and every tenth, make the news.
        let n = self.corps.corps[ci].bought_total;
        if n <= 3 || n % 10 == 0 {
            let (bname, pos) = (self.buildings[bid].name.clone(), self.lots.get(self.buildings[bid].lot).map_or(Vec2::ZERO, |l| l.center));
            self.social.push_news(tick, format!("🏢 {name} bought {bname}."), pos, None);
        }
        true
    }

    /// A branded business (or block of staff homes) on vacant land the corp
    /// bought: what the land is zoned for, at the land's tier, no taller
    /// than its neighbours allow. Names, floors and kinds from hashes.
    pub(super) fn corp_build_on(&mut self, lid: LotId, ci: usize) -> Option<BuildingId> {
        let lot = self.lots.get(lid)?;
        let lu = lot.land_use;
        let car = lot.car_access();
        let value = self.lot_value(lid);
        let tier = Tier::for_land_value(value);
        let h = mix(self.config.seed ^ 0xC0_B1, lid.data().as_ffi() ^ self.clock.tick);
        let kind = match lu {
            LandUse::Residential => None,
            LandUse::Commercial => {
                let opts = [BusinessKind::Office, BusinessKind::GadgetShop, BusinessKind::Grocery, BusinessKind::Bakery, BusinessKind::Hotel, BusinessKind::Cafe, BusinessKind::Bookshop];
                Some(self.corp_new_kind(ci, &opts, h))
            }
            // [sweep-sim] Not a row of "<Corp> Venue"s: a bar, a driving
            // range or a café, what it does not run yet first.
            LandUse::Entertainment => Some(self.corp_new_kind(ci, &[BusinessKind::Venue, BusinessKind::GolfRange, BusinessKind::Cafe], h)),
            LandUse::Industrial => Some(match self.corps.corps[ci].origin {
                k if k.industry().is_some() && !k.needs_site() => k,
                _ => [BusinessKind::Fab, BusinessKind::Assembly, BusinessKind::Mill][(h % 3) as usize],
            }),
            LandUse::Civic | LandUse::Park => return None,
        };
        let cap = self.height_cap(lid).max(1);
        let mut floors = match (lu, tier) {
            (LandUse::Industrial, _) => 2,
            (_, Tier::Low) => 3,
            (_, Tier::Mid) => 6,
            (_, Tier::High) => 12,
        }
        .min(cap);
        if let Some(m) = kind.and_then(|k| k.max_floors()) {
            floors = floors.min(m);
        }
        let short = self.corps.corps[ci].brand.name.split_whitespace().next().unwrap_or("Corp").to_string();
        let name = match (lu, kind) {
            (LandUse::Residential, _) => format!("{short} Residences"),
            // [sweep-sim] A bar is named like one.
            (_, Some(BusinessKind::Venue)) => format!("{short} {}", ["Lounge", "Club", "Social", "Taproom", "Hall"][(mix(h, 7) % 5) as usize]),
            (_, Some(k)) => format!("{short} {}", title_case(k.label())),
            _ => format!("{short} Building"),
        };
        let seed = self.fresh_seed();
        let mut b = Building::new(lid, lu, floors.max(1), tier, seed, name, self.clock.tick, value);
        if !car {
            b.parking_capacity = 0;
        }
        b.recompute_capacity();
        let jobs = b.jobs;
        b.business = kind.map(|k| Business::new(k, jobs));
        b.construction = Some(Construction::new(Construction::days_for(b.floors, tier)));
        b.recompute_capacity();
        b.update_rent(value);
        let id = self.buildings.insert(b);
        self.lots[lid].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        self.order_materials(id);
        let owner = self.corps.corps[ci].owner;
        self.give_to(id, owner, 0.0);
        Some(id)
    }

    /// [sweep-sim] Of `opts`, the first from a hash-picked start that corp
    /// `ci` does not run yet (the start itself when it runs them all).
    fn corp_new_kind(&self, ci: usize, opts: &[BusinessKind], h: u64) -> BusinessKind {
        let owned: Vec<BusinessKind> = self.estate.owners.get(self.corps.corps[ci].owner).map(|o| o.properties.iter().filter_map(|p| self.buildings.get(*p)?.business.as_ref().map(|z| z.kind)).collect()).unwrap_or_default();
        let start = (h % opts.len() as u64) as usize;
        (0..opts.len()).map(|k| opts[(start + k) % opts.len()]).find(|k| !owned.contains(k)).unwrap_or(opts[start])
    }

    /// Over the player's cap (it was lowered): sell the newest holdings
    /// (never the HQ) back to independent owners, two a day.
    pub(super) fn divest(&mut self, ci: usize) {
        let owner = self.corps.corps[ci].owner;
        let cap = self.corp_cap();
        let hq = self.corps.corps[ci].hq;
        for _ in 0..2 {
            let Some(o) = self.estate.owners.get(owner) else { return };
            if o.properties.len() <= cap {
                return;
            }
            let Some(&sell) = o.properties.iter().rev().find(|p| Some(**p) != hq) else { return };
            if let Some(o) = self.estate.owners.get_mut(owner) {
                o.properties.retain(|p| *p != sell);
            }
            if let Some(b) = self.buildings.get_mut(sell) {
                b.plot.owner = None;
            }
            self.corps.stats.divested_total += 1;
            self.events.push(SimEvent::BuildingChanged(sell));
        }
    }

    // ── staff housing ──────────────────────────────────────────────────────

    /// Staff living outside the corp's housing move into a free home in it
    /// (nearest their work first), a few a day.
    pub(super) fn house_staff(&mut self, ci: usize) {
        let owner = self.corps.corps[ci].owner;
        let (_, blocks) = self.corp_homes(owner);
        let mut free: Vec<BuildingId> = blocks.into_iter().filter(|b| self.buildings[*b].operating() && self.buildings[*b].free_homes() > 0).collect();
        if free.is_empty() {
            return;
        }
        let staff = self.corp_staff(owner);
        let mut moved = 0;
        let mut done: Vec<HouseholdId> = Vec::new();
        for cid in staff {
            if moved >= HOUSING_MOVES_PER_DAY || free.is_empty() {
                break;
            }
            let Some(c) = self.citizens.get(cid) else { continue };
            let Some(hid) = c.household else { continue };
            if done.contains(&hid) {
                continue;
            }
            done.push(hid);
            let home = self.households.get(hid).and_then(|h| h.home);
            if home.is_some_and(|h| self.buildings.get(h).is_some_and(|b| b.plot.owner == Some(owner))) {
                continue;
            }
            // Owner-occupiers and billionaires stay where they are.
            if home.is_some_and(|h| self.buildings.get(h).is_some_and(|b| b.plot.form.is_some_and(|f| f.owner_occupied()))) || self.households[hid].members.iter().any(|m| self.estate.is_elite(*m)) {
                continue;
            }
            let income = self.household_income(hid);
            let size = self.households[hid].members.len();
            let work = c.work.and_then(|w| self.building_lot(w)).map_or(Vec2::ZERO, |l| l.center);
            let Some(k) = (0..free.len())
                .filter(|k| self.buildings[free[*k]].rent * (1.0 - STAFF_RENT_DISCOUNT) <= income * trips::AFFORDABLE_SHARE.max(0.3) && (size <= 4 || self.buildings[free[*k]].free_homes() > 0))
                .min_by(|a, b| {
                    let da = self.building_lot(free[*a]).map_or(f32::MAX, |l| l.center.distance(work));
                    let db = self.building_lot(free[*b]).map_or(f32::MAX, |l| l.center.distance(work));
                    da.total_cmp(&db).then(free[*a].cmp(&free[*b]))
                })
            else {
                continue;
            };
            let to = free[k];
            let corp = self.corps.corps[ci].brand.name.clone();
            self.move_household(hid, to, &format!("Moved into {corp} staff housing at {{}}. Cheaper with the staff discount."));
            moved += 1;
            if self.buildings[to].free_homes() == 0 {
                free.remove(k);
            }
        }
        self.corps.corps[ci].housed_total += moved as u32;
        self.corps.stats.housed_today += moved as u32;
    }

    /// [megacorp] Dev: the corp with owner `owner` buys what it would buy
    /// next, now (cash topped up). Its index and what it bought.
    pub fn corp_buy_now(&mut self, owner: OwnerId) -> Result<Option<BuildingId>, String> {
        let ci = self.corp_index(owner).ok_or("no such corp")?;
        // Dev: the deposit is waived, the purse left as it was.
        let keep = self.corps.corps[ci].cash;
        self.corps.corps[ci].cash = 1e12;
        let before: Vec<BuildingId> = self.estate.owners.get(owner).map(|o| o.properties.clone()).unwrap_or_default();
        let bought = self.corp_buy(ci);
        self.corps.corps[ci].cash = keep;
        if !bought {
            return Ok(None);
        }
        Ok(self.estate.owners.get(owner).and_then(|o| o.properties.iter().copied().find(|p| !before.contains(p))))
    }

    /// [megacorp] Dev: the corp's HQ now if it has none, and its building
    /// sites finished (the HQ included).
    pub fn corp_finish_now(&mut self, owner: OwnerId) -> Result<usize, String> {
        let ci = self.corp_index(owner).ok_or("no such corp")?;
        if self.corps.corps[ci].hq.is_none() {
            self.build_hq(ci);
        }
        let sites: Vec<BuildingId> = self.estate.owners.get(owner).map(|o| o.properties.iter().copied().filter(|p| self.buildings.get(*p).is_some_and(|b| b.construction.is_some())).collect()).unwrap_or_default();
        for s in &sites {
            self.finish_construction(*s);
        }
        Ok(sites.len())
    }

    /// [megacorp] The player's rules: whether corps may form, the most one
    /// may own and the levy. Values are clamped to their ranges.
    pub fn set_corp_rules(&mut self, allowed: Option<bool>, max_share: Option<f32>, levy: Option<f32>) {
        let r = &mut self.corps.rules;
        if let Some(a) = allowed {
            r.allowed = a;
        }
        if let Some(s) = max_share {
            r.max_share = s.clamp(MIN_MAX_SHARE, MAX_MAX_SHARE);
        }
        if let Some(l) = levy {
            r.levy = l.clamp(0.0, MAX_LEVY);
        }
    }
}

fn buy_key(b: &Buy) -> (u8, u64) {
    match b {
        Buy::Building(id) => (0, id.data().as_ffi()),
        Buy::Land(id) => (1, id.data().as_ffi()),
    }
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
