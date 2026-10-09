//! [land] The land market on the `World`: the pressure grid sampled hourly,
//! sale prices and owners, developers buying out low buildings where the
//! city presses for more, firms opening branches or buying the plots next
//! door, families merging two plots for a McMansion, and the super-elite:
//! rare billionaires with mansions, NIMBY pull and escorted convoys.
//!
//! Costs: one pass over citizens and cars an hour, one over buildings and
//! lots a day, the convoys (a handful of cars) every tick. Draws come from
//! hash-seeded generators (`erng`), never the world's own stream, so the
//! rest of the simulation rolls the same dice as before.

use super::*;
use crate::estate::*;
use std::borrow::Cow;
use crate::vehicle::VehicleModel;

mod density;
mod owners;
mod growth;
mod elite;
mod convoys;

/// Fictional property developers.
const DEVELOPERS: &[&str] = &["Kessler Land", "Halcyon Developments", "Argent Holdings", "Meridian Estates", "Nocturne Property", "Zenith Build", "Fulcrum Realty"];

/// [review] The salts of [`World::erng`]'s small-number draw sites, in one
/// place so no two share one: `invest_rich` used 4, the same as
/// `daily_elites`, so on the same midnight tick both drew identical
/// numbers (an elite spawn day was always an invest day).
pub(super) mod erng_salt {
    pub const DENSIFY: u64 = 1;
    pub const EXPAND_FIRMS: u64 = 2;
    pub const MCMANSIONS: u64 = 3;
    pub const ELITES: u64 = 4;
    pub const NIMBY: u64 = 5;
    pub const INVEST_RICH: u64 = 6;
    pub const ACQUIRE_NAME: u64 = 9;
    #[cfg(test)]
    pub const ALL: [u64; 7] = [DENSIFY, EXPAND_FIRMS, MCMANSIONS, ELITES, NIMBY, INVEST_RICH, ACQUIRE_NAME];
}

impl World {
    /// A generator for one draw site, keyed by the tick: deterministic and
    /// independent of the world's own stream. Small salts come from
    /// [`erng_salt`]; the others are distinct hex tags.
    pub(super) fn erng(&self, salt: u64) -> SimRng {
        crate::rng_for(self.stream_seed(Stream::Estate) ^ salt.wrapping_mul(0x51_7C_C1_B7), self.clock.tick)
    }

    // ── the day ────────────────────────────────────────────────────────────

    /// Once a day, after the daily update: mobility, owners' books, buyouts
    /// where the city presses for more, firms growing, families and
    /// billionaires buying, NIMBY fading.
    pub(super) fn daily_estate(&mut self) {
        crate::prof_scope!("daily_estate");
        self.estate.stats.buyouts_today = 0;
        self.prune_estate();
        self.daily_mobility();
        self.densify();
        self.expand_firms();
        self.build_mcmansions();
        // [rich] The well-off buy a business or a second home.
        self.invest_rich();
        // [sweep-sim] ... and what they hold pays them.
        self.pay_investors();
        self.weekend_cars();
        self.daily_elites();
        self.daily_nimby();
    }

    /// Drop what no longer exists: owners' sold or demolished properties,
    /// plots pointing at buildings that do not span them, reservations of
    /// buildings that are gone.
    pub(super) fn prune_estate(&mut self) {
        let buildings = &self.buildings;
        for o in self.estate.owners.values_mut() {
            o.properties.retain(|p| buildings.contains_key(*p));
        }
        // Owners of nothing (and nobody's billionaire) go.
        // Founders named only on a business's books (older saves) get
        // their owner record.
        let named: Vec<(BuildingId, CitizenId)> = self.buildings.iter().filter(|(_, b)| b.plot.owner.is_none()).filter_map(|(id, b)| b.business.as_ref().and_then(|z| z.owner).filter(|c| self.citizens.contains_key(*c)).map(|c| (id, c))).collect();
        for (b, c) in named {
            self.set_business_owner(b, c);
        }
        // [review] A billionaire who died: their mansion and firms go on the
        // market, the neighbours' NIMBY leaves with them and the police
        // escort moves on. (The keep-list was built before the dead were
        // pruned, so their owner record held everything forever.)
        let citizens = &self.citizens;
        let dead: Vec<(OwnerId, Option<BuildingId>)> = self.estate.elites.iter().filter(|e| !citizens.contains_key(e.citizen)).map(|e| (e.owner, e.mansion)).collect();
        for (o, mansion) in &dead {
            let props = self.estate.owners.get_mut(*o).map(|x| std::mem::take(&mut x.properties)).unwrap_or_default();
            for b in props {
                if let Some(b) = self.buildings.get_mut(b).filter(|b| b.plot.owner == Some(*o)) {
                    b.plot.owner = None;
                }
            }
            if let Some(m) = mansion {
                self.estate.nimby.retain(|n| n.mansion != *m);
            }
        }
        if !dead.is_empty() {
            let citizens = &self.citizens;
            self.estate.elites.retain(|e| citizens.contains_key(e.citizen));
            self.refresh_police_escort();
        }
        let elites: Vec<OwnerId> = self.estate.elites.iter().map(|e| e.owner).collect();
        self.estate.owners.retain(|id, o| !o.properties.is_empty() || elites.contains(&id));
        for b in self.buildings.values_mut() {
            if b.plot.owner.is_some_and(|o| !self.estate.owners.contains_key(o)) {
                b.plot.owner = None;
            }
        }
        let lots = &self.lots;
        let buildings = &self.buildings;
        self.estate.reserved.retain(|l, b| lots.contains_key(*l) && buildings.contains_key(*b));
        // Every lot naming a building must be its primary or one of its plots.
        let mut stray: Vec<LotId> = Vec::new();
        for (lid, l) in self.lots.iter() {
            let Some(bid) = l.building else { continue };
            match self.buildings.get(bid) {
                Some(b) if b.lot == lid || b.plot.extra_lots.contains(&lid) => {}
                _ => stray.push(lid),
            }
        }
        for lid in stray {
            self.lots[lid].building = None;
        }
        let lots = &self.lots;
        for (bid, b) in self.buildings.iter_mut() {
            b.plot.extra_lots.retain(|l| lots.get(*l).is_some_and(|x| x.building == Some(bid)));
        }
        let cars = &self.traffic.cars;
        let citizens = &self.citizens;
        self.estate.elites.retain(|e| citizens.contains_key(e.citizen));
        for c in &mut self.estate.convoys {
            c.escorts.retain(|e| cars.contains_key(*e));
            c.police = c.police.filter(|p| cars.contains_key(*p));
        }
        let tick = self.clock.tick;
        self.estate.convoys.retain(|c| cars.contains_key(c.boss) || !c.escorts.is_empty() || c.police.is_some() || c.stop.is_some_and(|(_, t)| t > tick));
        // [rich] Garages of people who left are sold off.
        self.prune_spare_cars();
    }
}

/// Daily income of a fortune: enough to stand out of any income table
/// without swamping the town's books.
fn elite_income(net_worth: f64) -> f32 {
    (2_000.0 + (net_worth / 1e9) as f32 * 120.0).min(8_000.0)
}

/// A billionaire's supercar: a souped-up, glossy coupe.
pub fn supercar(seed: u64) -> VehicleModel {
    use crate::vehicle::{BodyType, Livery, VehicleClass, PAINTS};
    VehicleModel { class: VehicleClass::Expensive, body: BodyType::Coupe, wear: 0, tuned: true, livery: Livery::None, paint: (seed % PAINTS as u64) as u8 }
}

/// The black SUVs of an escort.
pub fn escort_model() -> VehicleModel {
    use crate::vehicle::{BodyType, Livery, VehicleClass};
    VehicleModel { class: VehicleClass::Expensive, body: BodyType::Suv, wear: 0, tuned: false, livery: Livery::None, paint: 0 }
}

/// The police car leading the richest one's convoy (the renderer adds the
/// light bar: `render/convoys.rs`).
pub fn police_escort_model() -> VehicleModel {
    use crate::vehicle::{BodyType, Livery, VehicleClass};
    VehicleModel { class: VehicleClass::Mid, body: BodyType::Sedan, wear: 0, tuned: false, livery: Livery::None, paint: 3 }
}

#[cfg(test)]
mod salt_tests {
    #[test]
    fn every_estate_draw_site_has_its_own_salt() {
        let all = super::erng_salt::ALL;
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "salt {a} used twice");
        }
    }
}
