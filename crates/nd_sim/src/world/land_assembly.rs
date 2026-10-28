//! [land-assembly] Companies assemble land behind the road. Any parcel can
//! be zoned, backland with no road frontage too (`Parcel::refusal`); a
//! zoned plot is built on only if it fronts a road, or touches a plot whose
//! building the same company owns and is joined to it
//! (`SiteRule::backland`, `World::plots_next_to`). A company on a road plot
//! buys the plots round it through the land market and spreads over them as
//! one site, drawn as one (`World::footprint`) and entered by its road plot:
//!
//! - open-ground trades grow in place without closing
//!   ([`crate::estate::grows_in_place`]): a strip mine digs its pit wider and
//!   deeper, an oil field takes more wells, farms and feedlots more fields
//!   and pens, a car park more bays; they buy empty plots only (backland
//!   first), up to [`crate::estate::BIG_SITE_LOTS`];
//! - the rest of industry buys the plots next door, built or empty, and
//!   rebuilds bigger across them (`World::expand_into_neighbours`), up to
//!   [`crate::estate::INDUSTRY_SITE_LOTS`].
//!
//! Empty land is bought at its value (the same land term as
//! `World::sale_price`): the company puts [`LAND_DEPOSIT`] of it down, the
//! bank the rest (out of town, like every purchase).
//!
//! No draws from the world's stream: the daily pass picks by hash.

use super::*;
use crate::estate::{grows_in_place, merged_lot_within, site_cap, Form, SITE_HULL_SLACK};
use crate::hash::hash01;

/// Share of an empty plot's price the company pays itself.
pub const LAND_DEPOSIT: f64 = 0.1;
/// Plots an open-ground site buys at a time.
pub const SITE_PLOTS_AT_ONCE: usize = 2;
/// Buildings at most this tall on a plot it wants are bought and pulled
/// down (a shed, a yard, a two-storey works).
pub const SITE_MAX_FLOORS_BOUGHT: u8 = 2;
/// Sites that grow in a day across the city, at most, and the chance a day
/// one that wants land looks.
pub const SITES_PER_DAY: usize = 2;
pub const SITE_DAILY_CHANCE: f32 = 0.15;
/// A site wants land after this many days open...
pub const SITE_MIN_DAYS: u16 = 7;
/// ... while its trade pays its way (takings over costs, the market's
/// reading of the kind) at least this much.
pub const SITE_MIN_COVER: f32 = 1.0;

impl World {
    /// [land-assembly] What an empty plot sells for: its land at its value
    /// (the land term of `sale_price`).
    pub fn land_price(&self, lot: LotId) -> f64 {
        let Some(l) = self.lots.get(lot) else { return 0.0 };
        l.area() as f64 * (25.0 + 400.0 * self.lot_value(lot).max(0.0) as f64)
    }

    /// [land-assembly] An open-ground business that grows in place
    /// (`grows_in_place`; a car park only as a surface lot).
    pub fn grows_over_land(&self, bid: BuildingId) -> bool {
        self.buildings.get(bid).and_then(|b| b.business.as_ref().map(|z| (b, z.kind))).is_some_and(|(b, k)| grows_in_place(k) && (k != BusinessKind::Parking || b.floors == 0))
    }

    /// [land-assembly] The open-ground business at `bid` buys up to
    /// [`SITE_PLOTS_AT_ONCE`] empty plots touching its site (backland
    /// first, then the cheapest) that keep it one compact site, and spreads
    /// over them while it stays open. True if it bought.
    pub fn grow_site_in_place(&mut self, bid: BuildingId) -> bool {
        if !self.grows_over_land(bid) {
            return false;
        }
        let b = &self.buildings[bid];
        if !b.operating() || !b.plot.pending_lots.is_empty() || b.plot.tower.is_some() || b.is_tenant() {
            return false;
        }
        let kind = b.business.as_ref().map_or(BusinessKind::Parking, |z| z.kind);
        let ours = b.plot.lots(b.lot);
        let cap = site_cap(kind);
        if ours.len() >= cap {
            return false;
        }
        let lu = self.lots.get(b.lot).map_or(b.land_use, |l| l.land_use);
        let owner = b.plot.owner;
        // Empty plots first (backland first among them), then low buildings
        // the market may buy, which come down (a car park takes empty
        // ground only).
        let built_ok = kind != BusinessKind::Parking;
        let mut cands: Vec<(bool, bool, f64, LotId)> = self
            .plots_next_to(&ours, lu, SITE_MAX_FLOORS_BOUGHT, owner)
            .into_iter()
            .filter(|(_, l)| built_ok || self.lots[*l].building.is_none())
            .map(|(price, l)| {
                let built = self.lots[l].building.is_some();
                (built, !self.lots[l].landlocked, if built { price } else { self.land_price(l) }, l)
            })
            .collect();
        cands.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)).then(a.3.cmp(&b.3)));
        let mut picked: Vec<LotId> = Vec::new();
        let mut price = 0.0f64;
        for (_, _, p, lid) in cands {
            if picked.len() >= SITE_PLOTS_AT_ONCE || ours.len() + picked.len() >= cap {
                break;
            }
            let all: Vec<&Lot> = ours.iter().skip(1).chain(picked.iter()).chain(std::iter::once(&lid)).filter_map(|l| self.lots.get(*l)).collect();
            if merged_lot_within(&self.lots[ours[0]], &all, SITE_HULL_SLACK).is_none() {
                continue;
            }
            // One piece: every plot joined to the rest along an edge.
            let polys: Vec<Vec<Vec2>> = std::iter::once(&self.lots[ours[0]]).chain(all.iter().copied()).map(|l| l.polygon.clone()).collect();
            let area: f32 = polys.iter().map(|p| crate::geom::polygon_area(p).abs()).sum();
            if crate::zoning::joined_outline(&polys).is_none_or(|u| crate::geom::polygon_area(&u).abs() < area * 0.97) {
                continue;
            }
            picked.push(lid);
            price += p;
        }
        if picked.is_empty() {
            return false;
        }
        let deposit = (price * LAND_DEPOSIT) as f32;
        let money = self.buildings[bid].business.as_ref().map_or(0.0, |z| z.money);
        if money < deposit + 200.0 {
            return false;
        }
        let firm = self.firm_of(bid);
        let built = picked.iter().filter(|l| self.lots[**l].building.is_some()).count();
        // [ledger] The deposit to the sellers out of town; the bank lends the rest.
        self.transfer(Account::Business(bid), Account::Outside, deposit, Kind::PropertySale);
        let backland = picked.iter().filter(|l| self.lots[**l].landlocked).count();
        let name = self.buildings[bid].name.clone();
        self.take_plots(bid, &picked, &format!("{name} bought the plot to spread over it."));
        let b = &mut self.buildings[bid];
        if b.plot.form.is_none() {
            b.plot.form = Some(Form::Merged);
        }
        // A surface car park has the bays its whole site fits.
        if kind == BusinessKind::Parking {
            if let Some(cap) = self.footprint(bid).map(|l| crate::parking_layout::surface_capacity(&l).max(1)) {
                let b = &mut self.buildings[bid];
                b.parking_capacity = cap.max(b.parking_capacity);
            }
        }
        if let Some(o) = firm.filter(|o| self.estate.owners.contains_key(*o)) {
            self.estate.owners[o].spent += price;
            let n = 1 + self.buildings[bid].plot.extra_lots.len() + self.buildings[bid].plot.pending_lots.len();
            let what = match (backland, built) {
                (0, 0) => format!("bought {} plots beside {name} ({n} in all)", picked.len()),
                (0, _) => format!("bought out {built} neighbours of {name} to spread over their plots ({n} in all)"),
                _ => format!("bought {} plots behind {name} ({n} in all)", picked.len()),
            };
            self.note_owner(o, what);
        }
        self.estate.stats.site_plots_total += picked.len() as u32;
        self.estate.stats.backland_plots_total += backland as u32;
        self.events.push(SimEvent::BuildingChanged(bid));
        true
    }

    /// [land-assembly] Does the open-ground business at `bid` want more
    /// land: open a week, mostly staffed, not losing money these three
    /// days, its trade paying its way, cash for a deposit?
    fn site_wants_land(&self, bid: BuildingId) -> bool {
        let Some(b) = self.buildings.get(bid) else { return false };
        let Some(z) = &b.business else { return false };
        if z.closed || !b.operating() || b.is_derelict() || z.days_open < SITE_MIN_DAYS || z.books.len() < 3 {
            return false;
        }
        if b.jobs > 0 && b.staffing() < 0.6 {
            return false;
        }
        let last = &z.books[z.books.len() - 3..];
        if !last.iter().all(|d| d.revenue >= d.spend) {
            return false;
        }
        self.economy.cover_of(z.kind).is_none_or(|c| c >= SITE_MIN_COVER) && z.money >= 400.0
    }

    /// [land-assembly] Once a day: open-ground businesses that want land
    /// (a hash pick a day, [`SITES_PER_DAY`] at most) buy the plots round
    /// them.
    pub(super) fn assemble_sites(&mut self) {
        let day = self.clock.day();
        let sites: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| !b.is_special() && b.business.as_ref().is_some_and(|z| grows_in_place(z.kind))).map(|(id, _)| id).collect();
        let mut grown = 0;
        for bid in sites {
            if grown >= SITES_PER_DAY {
                break;
            }
            let seed = self.buildings[bid].seed;
            if hash01(seed, day, 0x51_7E) >= SITE_DAILY_CHANCE || !self.site_wants_land(bid) {
                continue;
            }
            if self.grow_site_in_place(bid) {
                grown += 1;
            }
        }
    }

    /// [land-assembly] For probes: each open-ground business, whether it
    /// wants land, the empty plots touching it and the deposit they ask
    /// against its cash.
    #[doc(hidden)]
    pub fn site_growth_diag(&self) -> Vec<String> {
        let mut v = Vec::new();
        for (bid, b) in self.buildings.iter() {
            let Some(z) = b.business.as_ref().filter(|z| grows_in_place(z.kind)) else { continue };
            if b.is_special() {
                continue;
            }
            let ours = b.plot.lots(b.lot);
            let lu = self.lots.get(b.lot).map_or(b.land_use, |l| l.land_use);
            let empty: Vec<LotId> = self.plots_next_to(&ours, lu, 0, b.plot.owner).into_iter().filter(|(_, l)| self.lots[*l].building.is_none()).map(|(_, l)| l).collect();
            let deposit: f64 = empty.iter().take(SITE_PLOTS_AT_ONCE).map(|l| self.land_price(*l)).sum::<f64>() * LAND_DEPOSIT;
            let last: Vec<String> = z.books.iter().rev().take(3).map(|d| format!("{:.0}/{:.0}", d.revenue, d.spend)).collect();
            v.push(format!("{} [{}] plots {} wants {} empty next {} deposit {:.0} cash {:.0} cover {:?} open {} books {}", b.name, z.kind.label(), ours.len(), self.site_wants_land(bid), empty.len(), deposit, z.money, self.economy.cover_of(z.kind).map(|c| (c * 100.0).round() / 100.0), z.days_open, last.join(" ")));
        }
        v
    }

    /// [malls] For probes: commercial road plots wide and deep enough for
    /// a strip mall (and of them vacant), and big enough for a mall.
    #[doc(hidden)]
    pub fn mall_site_diag(&self) -> (usize, usize, usize, usize, usize) {
        let cands = self.buildings.iter().filter(|(id, b)| self.mall_rebuild_candidate(*id, b)).count();
        let (mut wide, mut vacant, mut big, mut all) = (0, 0, 0, 0);
        for l in self.lots.values().filter(|l| l.land_use == LandUse::Commercial && !l.landlocked && l.car_access()) {
            all += 1;
            let frontage = l.front_edge[0].distance(l.front_edge[1]);
            let depth = l.corners[0].distance(l.corners[3]);
            if frontage >= super::malls::STRIP_MALL_MIN_FRONTAGE && depth >= super::malls::STRIP_MALL_MIN_DEPTH {
                wide += 1;
                if l.building.is_none() {
                    vacant += 1;
                }
            }
            if l.area() >= super::malls::MALL_MIN_AREA {
                big += 1;
            }
        }
        (all, wide, vacant, big, cands)
    }

    /// [land-assembly] For probes and screenshots: `kind` opens on `lot` at
    /// once, whatever stood there pulled down first (a car park as the
    /// market would open one).
    #[doc(hidden)]
    pub fn force_open_site(&mut self, lot: LotId, kind: BusinessKind) -> Option<BuildingId> {
        if let Some(old) = self.lots.get(lot)?.building {
            self.demolish(old);
            self.finish_demolition(old);
            if self.buildings.contains_key(old) {
                return None;
            }
        }
        let id = if kind == BusinessKind::Parking { self.build_parking(lot)? } else { self.build_on_as(lot, Some(kind))? };
        if self.buildings[id].construction.is_some() {
            self.finish_construction(id);
        }
        if kind == BusinessKind::Parking && self.buildings[id].floors > 0 {
            self.buildings[id].floors = 0;
            self.buildings[id].parking_capacity = self.footprint(id).map_or(4, |l| crate::parking_layout::surface_capacity(&l).max(4));
        }
        Some(id)
    }
}
