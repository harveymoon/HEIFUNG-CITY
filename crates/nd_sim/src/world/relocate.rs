//! [relocate] A business bought out of its building moves instead of only
//! closing. Whatever takes the building (road works through it, a city
//! building put on its lot, a developer's rebuild, a neighbour buying the
//! plot to grow, a billionaire's estate) first packs the business up
//! (`pack_business`): its cash, books, stock, owner and staff go into
//! `EstateMarket::moving`. The next development round (`relocate_waiting`)
//! finds it the nearest vacant lot of its land use within
//! [`RELOCATE_REACH`] with at least half its old ground, and if it can pay
//! the move ([`move_cost`]) the same business, under the same name and
//! owner, opens a building site there; its old staff are hired back when
//! the new building opens. A business that cannot afford it, or finds no
//! lot within [`MOVE_WAIT_TICKS`], is gone, as before.
//!
//! No draws of its own: the lot is the nearest (ties by key) and the new
//! building's name is the old one. (`build_on_named` picks the new
//! building's floors from the world's stream, as for any new building.)

use super::*;
use crate::estate::{Moving, OwnerKind};
use crate::industry::IndustryKind;

/// How far a business will move, m.
pub const RELOCATE_REACH: f32 = 1500.0;
/// How long a packed-up business looks for a lot before it gives up: two days.
pub const MOVE_WAIT_TICKS: u64 = 2 * 1440;

/// What a move costs a business of `jobs` jobs: the vans, the fit-out of
/// the new place, the days without takings.
pub fn move_cost(jobs: u16) -> f32 {
    200.0 + 20.0 * jobs as f32
}

/// Kinds that are tied to their ground (fields, wells, the water) or are
/// the city's own kind of place: they close rather than move.
fn rooted(kind: BusinessKind) -> bool {
    matches!(kind, BusinessKind::Parking | BusinessKind::TaxiCompany | BusinessKind::CargoHub | BusinessKind::Marina | BusinessKind::FishingPort | BusinessKind::StripMine)
        || kind.industry().is_some_and(|k| matches!(k, IndustryKind::Farming | IndustryKind::OilExtraction | IndustryKind::Fishing))
}

impl World {
    /// `bid` is about to lose its building to a buyout (`why`: "road
    /// works", ...): an open business that could pay a move is packed up
    /// to look for a new place. Call before the building is taken.
    pub(super) fn pack_business(&mut self, bid: BuildingId, why: &str) {
        let Some(b) = self.buildings.get(bid) else { return };
        let Some(lot) = self.lots.get(b.lot).filter(|l| l.building == Some(bid)) else { return };
        let (from, area, lu) = (lot.center, lot.area(), lot.land_use);
        self.pack_business_at(bid, why, from, area, lu);
    }

    /// [`World::pack_business`] for a building whose lot is already gone
    /// (road works): where it stood, its ground and land use given.
    pub(super) fn pack_business_at(&mut self, bid: BuildingId, why: &str, from: Vec2, area: f32, land_use: LandUse) {
        let Some(b) = self.buildings.get(bid) else { return };
        if !b.operating() || b.is_special() {
            return;
        }
        let Some(z) = b.business.as_ref() else { return };
        if z.closed || rooted(z.kind) || z.money < move_cost(b.jobs) {
            return;
        }
        if self.estate.moving.iter().any(|m| m.name == b.name && m.business.kind == z.kind) {
            return;
        }
        let owner = b.plot.owner.filter(|o| self.estate.owners.get(*o).is_some_and(|o| o.kind != OwnerKind::Developer));
        let staff: Vec<CitizenId> = b.workers.iter().copied().filter(|c| self.citizens.get(*c).is_some_and(|c| c.work == Some(bid))).collect();
        // Its old address by the nearest neighbour still standing.
        let from_words = self
            .buildings
            .iter()
            .filter(|(id, x)| *id != bid && x.operating())
            .filter_map(|(_, x)| self.lots.get(x.lot).map(|l| (l.center.distance(from), x.name.as_str())))
            .filter(|(d, _)| *d < 150.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or_else(|| "its old place".to_string(), |(_, n)| format!("its old place next to {n}"));
        let m = Moving {
            business: z.clone(),
            name: b.name.clone(),
            land_use,
            from,
            area,
            jobs: b.jobs,
            owner,
            staff,
            why: why.to_string(),
            from_words,
            since_tick: self.clock.tick,
        };
        self.estate.moving.push(m);
    }

    /// Every packed-up business tries to move (oldest first); those that
    /// find a place open a building site there, those that waited too long
    /// are gone. Called each development round.
    pub(super) fn relocate_waiting(&mut self) {
        if self.estate.moving.is_empty() {
            return;
        }
        let waiting = std::mem::take(&mut self.estate.moving);
        let tick = self.clock.tick;
        for m in waiting {
            if self.relocate(&m).is_some() {
                self.estate.stats.relocated_total += 1;
            } else if tick.saturating_sub(m.since_tick) < MOVE_WAIT_TICKS {
                self.estate.moving.push(m);
            } else {
                self.estate.stats.relocate_failed_total += 1;
            }
        }
    }

    /// The vacant lot a business packed up at `m` would move to: its land
    /// use, at least half its old ground, a street out front, within
    /// [`RELOCATE_REACH`]; the nearest.
    pub fn relocation_lot(&self, m: &Moving) -> Option<LotId> {
        self.lots
            .iter()
            .filter(|(id, l)| {
                l.land_use == m.land_use && l.area() >= m.area * 0.5 && self.lot_free_for(*id, SiteRule::STREET)
            })
            .map(|(id, l)| (l.center.distance(m.from), id))
            .filter(|(d, _)| *d <= RELOCATE_REACH)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, id)| id)
    }

    /// Move one packed-up business: a site on the nearest suitable lot,
    /// the business (less the move) in it under its old name and owner,
    /// its old staff to be hired back, a line in the news.
    fn relocate(&mut self, m: &Moving) -> Option<BuildingId> {
        let cost = move_cost(m.jobs);
        if m.business.money < cost {
            return None;
        }
        let lot = self.relocation_lot(m)?;
        let new = self.build_on_named(lot, Some(m.business.kind), Some(m.name.clone()))?;
        let to = self.lots[lot].center;
        let old = if m.from_words.is_empty() { "its old place".to_string() } else { m.from_words.clone() };
        let from_words = format!("{old}, {:.0} m away, after {}", to.distance(m.from), m.why);
        let mut z = m.business.clone();
        z.broke_days = 0;
        z.closed = false;
        // Orders on the way went to the old address; lorries parked there
        // were sold with it.
        z.inbound.clear();
        z.fleet_trucks = 0;
        z.fleet_log.clear();
        z.moved_from = Some(from_words.clone());
        z.origin = Some(format!("Moved here after losing its old building to {}.", m.why));
        z.rehire = m.staff.clone();
        let owner_cid = z.owner;
        // [ledger] The till moves with it (it left with the old building
        // and comes back here), and the move is paid from it.
        self.replace_business(new, z);
        self.transfer(Account::Business(new), Account::Outside, cost, Kind::Building);
        match m.owner.filter(|o| self.estate.owners.contains_key(*o)) {
            Some(o) => self.give_to(new, o, 0.0),
            None => {
                if let Some(c) = owner_cid.filter(|c| self.citizens.contains_key(*c)) {
                    self.set_business_owner(new, c);
                }
            }
        }
        let tick = self.clock.tick;
        let far = to.distance(m.from);
        self.social.push_news(tick, format!("🚚 {} lost its building to {} and is moving {:.0} m away.", m.name, m.why, far), to, None);
        for &c in &m.staff {
            if let Some(c) = self.citizens.get_mut(c) {
                c.remember(tick, format!("{} lost its building to {}. It's moving; they'll take us back when it reopens.", m.name, m.why));
            }
        }
        Some(new)
    }

    /// [relocate] A moved business's building opened: its old staff who
    /// are still out of work come back. Called when construction finishes.
    pub(super) fn rehire_moved_staff(&mut self, bid: BuildingId) {
        let Some(staff) = self.buildings.get_mut(bid).and_then(|b| b.business.as_mut()).map(|z| std::mem::take(&mut z.rehire)) else { return };
        for c in staff {
            let ok = self.citizens.get(c).is_some_and(|x| x.work.is_none() && x.can_work());
            if ok && self.buildings[bid].free_jobs() > 0 {
                self.assign_job(c, bid);
            }
        }
    }
}
