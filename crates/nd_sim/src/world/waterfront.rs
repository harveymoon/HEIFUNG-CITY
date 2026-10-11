//! [economy] The waterfront: recreational marinas and fishing ports on
//! docks the player places (Build > Water). They stand on the shore like
//! the shipping ports and ferry terminals (`ports::Port::plan` finds the
//! waterline, sets the yard back on dry land and builds a pier out into
//! open water; a lake will do for these), and the city pays for the quay.
//! An operator runs each on its takings: a marina lets berths to the
//! wealthy households around (a large one also takes yachts from anywhere
//! in town and keeps a clubhouse, a place to go out), a fishing port lands
//! fish for the gutting sheds, the fish-stick factory and the fish
//! markets. An operator that goes under is replaced by a new one taking
//! the lease (the dock stays). Marinas raise the land value around them
//! and draw seafood restaurants (`crate::pressure`).
//!
//! Boats: sailing boats and motor yachts go out for a few hours by day.
//! They are ships of the port model (`ports::Ship`, not saved), drawn
//! moving; the rest lie at their berths (drawn with the dock,
//! `nd_procgen::terrain`). [fishing] Trawlers are the fishing fleets of
//! `crate::fishing` (`world/fishing.rs`): out to the grounds in the
//! morning, home in the afternoon, their berths empty while they are out.

use super::*;
use crate::ports::{Port, PortKind, Ship, ShipKind};

/// Daily fee for a small boat's berth, and a yacht's at a large marina.
pub const BERTH_FEE: f32 = 20.0;
pub const YACHT_FEE: f32 = 32.0;
/// Rich households keep a boat at a marina within this.
pub const MARINA_CATCHMENT: f32 = 1500.0;
/// The float a new operator starts with: boats and nets to buy.
pub const OPERATOR_FLOAT: f32 = 1500.0;
/// Marina boats out on the water at once, all docks together.
pub const MAX_BOATS: usize = 24;
/// Land value a marina lends the streets around: (radius, lift).
pub fn marina_lift(kind: PortKind) -> (f32, f32) {
    match kind {
        PortKind::MarinaLarge => (320.0, 0.14),
        _ => (220.0, 0.08),
    }
}

/// Minutes a boat stays at the far end of its trip: hauling nets, or at
/// anchor.
pub fn boat_dwell(kind: ShipKind) -> u32 {
    match kind {
        ShipKind::FishingBoat => 300,
        ShipKind::Yacht => 70,
        ShipKind::Sailboat => 40,
        _ => 0,
    }
}

fn hash(a: u64, b: u64) -> u64 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 29)
}

fn rotate(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

impl World {
    /// Put an operator on a dock: a fresh business of the dock's kind with
    /// its float. `again`: the last one went under.
    pub(super) fn lease_dock(&mut self, id: BuildingId, kind: PortKind, again: bool) {
        let Some(bk) = kind.business() else { return };
        let Some(b) = self.buildings.get_mut(id) else { return };
        let mut z = Business::new(bk, b.jobs);
        z.money = OPERATOR_FLOAT;
        z.origin = Some(if again {
            format!("A new operator took over the lease of this {} after the last one went under.", kind.label())
        } else {
            format!("The city built this {}; an operator runs it on its takings.", kind.label())
        });
        b.business = Some(z);
        b.condition = b.condition.max(0.8);
        self.events.push(SimEvent::BuildingChanged(id));
    }

    /// Midnight: closed operators replaced, berth fees collected.
    pub(super) fn waterfront_daily(&mut self) {
        let docks: Vec<(BuildingId, PortKind, Vec2)> = self.ports.ports.values().filter(|p| p.kind.business().is_some()).filter_map(|p| p.building.map(|b| (b, p.kind, p.center))).collect();
        for &(b, kind, _) in &docks {
            let relet = self.buildings.get(b).and_then(|x| x.business.as_ref()).is_none_or(|z| z.closed && z.broke_days >= 3);
            if relet && self.buildings.contains_key(b) {
                self.lease_dock(b, kind, true);
            }
        }
        // Rich households and where they live, in id order.
        let rich: Vec<(crate::citizens::HouseholdId, Vec2)> = self
            .households
            .iter()
            .filter(|(h, _)| self.household_class(*h) == Tier::High)
            .filter_map(|(h, hh)| hh.home.and_then(|b| self.building_lot(b)).map(|l| (h, l.center)))
            .collect();
        let mut fees_total = 0.0;
        // [pt-visual] Each billionaire keeps a superyacht at the large
        // marina nearest home (fictional people: the town's own elite).
        let large: Vec<(BuildingId, Vec2)> = docks.iter().filter(|(b, k, _)| *k == PortKind::MarinaLarge && self.buildings.get(*b).and_then(|x| x.business.as_ref()).is_some_and(|z| !z.closed)).map(|(b, _, at)| (*b, *at)).collect();
        let mut supers: std::collections::HashMap<BuildingId, u8> = std::collections::HashMap::new();
        if !large.is_empty() {
            for e in &self.estate.elites {
                let home = self.citizens.get(e.citizen).and_then(|c| c.home).or(e.mansion).and_then(|b| self.building_lot(b)).map(|l| l.center);
                let Some(home) = home else { continue };
                if let Some((b, _)) = large.iter().min_by(|x, y| x.1.distance(home).total_cmp(&y.1.distance(home)).then(x.0.cmp(&y.0))) {
                    let n = supers.entry(*b).or_insert(0);
                    *n = (*n + 1).min(crate::ports::MAX_SUPERYACHTS);
                }
            }
        }
        let mut moored: Vec<(BuildingId, crate::ports::Moored)> = Vec::new();
        for (b, kind, at) in docks {
            if !kind.is_marina() || self.buildings.get(b).and_then(|x| x.business.as_ref()).is_none_or(|z| z.closed) {
                if kind.is_marina() {
                    moored.push((b, crate::ports::Moored::default()));
                }
                continue;
            }
            let berths = kind.berths() as usize;
            // Owners near the water keep a small boat here; a large marina
            // also draws yachts from the rich anywhere in town.
            let mut near: Vec<(f32, crate::citizens::HouseholdId)> = rich.iter().map(|(h, p)| (p.distance(at), *h)).filter(|(d, _)| *d <= MARINA_CATCHMENT).collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let mut owners: Vec<(crate::citizens::HouseholdId, f32)> = near.into_iter().take(berths).map(|(_, h)| (h, BERTH_FEE)).collect();
            if kind == PortKind::MarinaLarge {
                let yachts = (rich.len() / 12).min(berths.saturating_sub(owners.len()));
                let mut far: Vec<crate::citizens::HouseholdId> = rich.iter().map(|(h, _)| *h).filter(|h| !owners.iter().any(|o| o.0 == *h)).collect();
                far.sort_by_key(|h| hash(slotmap::Key::data(h).as_ffi(), slotmap::Key::data(&b).as_ffi()));
                owners.extend(far.into_iter().take(yachts).map(|h| (h, YACHT_FEE)));
            }
            moored.push((b, crate::ports::Moored { berths_let: owners.len().min(berths) as u16, superyachts: supers.get(&b).copied().unwrap_or(0) }));
            let mut fees = 0.0;
            for (h, fee) in owners {
                if let Some(hh) = self.households.get_mut(h) {
                    if hh.money > fee * 5.0 {
                        hh.money -= fee;
                        fees += fee;
                    }
                }
            }
            if let Some(z) = self.buildings.get_mut(b).and_then(|x| x.business.as_mut()) {
                z.money += fees;
                z.revenue_today += fees;
            }
            fees_total += fees;
        }
        self.scene.berth_fees_today = fees_total;
        for p in self.ports.ports.values_mut() {
            if let Some(m) = p.building.and_then(|b| moored.iter().find(|(x, _)| *x == b)) {
                p.moored = Some(m.1);
            }
        }
    }

    /// Every ten minutes: pleasure boats go out by day, a few from each
    /// marina, never more than its berths.
    pub(super) fn step_boats(&mut self) {
        let tick = self.clock.tick;
        if tick % 10 != 6 || self.ports.ports.is_empty() {
            return;
        }
        let hour = self.clock.hour();
        let docks: Vec<(BuildingId, Port)> = self
            .ports
            .ports
            .values()
            .filter(|p| p.kind.is_marina() && p.works())
            .filter_map(|p| p.building.map(|b| (b, p.clone())))
            .filter(|(b, _)| self.buildings.get(*b).is_some_and(|x| x.operating() && x.business.as_ref().is_some_and(|z| !z.closed)))
            .collect();
        let boats = |w: &World| w.ports.ships.values().filter(|s| matches!(s.kind, ShipKind::Yacht | ShipKind::Sailboat)).count();
        // [fishing] Trawlers at sea count too.
        self.scene.boats_out = (boats(self) + self.fishing.fleets.values().map(|f| f.at_sea()).sum::<usize>()) as u32;
        for (b, port) in docks {
            if boats(self) >= MAX_BOATS {
                break;
            }
            let out = self.ports.ships.values().filter(|s| s.port == b).count() as u32;
            let (window, most) = (9..18, port.kind.berths().div_ceil(4));
            if !window.contains(&hour) || out >= most {
                continue;
            }
            let h = hash(slotmap::Key::data(&b).as_ffi(), tick);
            if h % 3 != 0 {
                continue;
            }
            let kind = if port.kind == PortKind::MarinaLarge && h % 5 < 2 {
                ShipKind::Yacht
            } else {
                ShipKind::Sailboat
            };
            let range = match kind {
                ShipKind::FishingBoat => (250.0, 700.0),
                ShipKind::Yacht => (200.0, 600.0),
                _ => (80.0, 300.0),
            };
            let Some(out_leg) = self.boat_path(&port, h >> 3, range) else { continue };
            let berth_s = crate::geom::path_length(&out_leg);
            let mut path = out_leg.clone();
            path.extend(out_leg.iter().rev().skip(1).copied());
            let mps = match kind {
                ShipKind::FishingBoat => 4.0,
                ShipKind::Yacht => 6.0,
                _ => 3.0,
            };
            let speed = self.config.metres_per_tick(mps);
            self.ports.ships.insert(Ship { kind, path, s: 0.0, prev_s: 0.0, speed, dwell: 5, port: b, to: None, cargo: Vec::new(), passengers: 0, docked: false, berth_s, color: (h % 6) as u8, remote: None, home: false, quay: None, boxes_in: 0, boxes_out: 0 });
        }
    }

    /// A boat's way out from the berth onto open water, `range` metres
    /// long at most, keeping clear of the banks: tried along a few
    /// headings (out from the shore, then along a river). `None` when the
    /// water is too cramped.
    pub(super) fn boat_path(&self, port: &Port, h: u64, range: (f32, f32)) -> Option<Vec<Vec2>> {
        let land = &self.landscape;
        let start = port.berth;
        let len = range.0 + (h % 1000) as f32 / 1000.0 * (range.1 - range.0);
        let wet = |p: Vec2, dir: Vec2| {
            let side = Vec2::new(-dir.y, dir.x) * 5.0;
            land.is_water(p) && land.is_water(p + side) && land.is_water(p - side)
        };
        let flip = if (h >> 11) % 2 == 0 { 1.0 } else { -1.0 };
        let headings = [0.0, 0.45 * flip, -0.45 * flip, 0.9 * flip, -0.9 * flip, 1.35 * flip, -1.35 * flip];
        for (k, a) in headings.iter().enumerate() {
            let dir = rotate(port.seaward, *a + ((h >> (k * 3)) % 7) as f32 * 0.02);
            let step = 12.0;
            let mut d = 0.0;
            while d + step <= len && wet(start + dir * (d + step), dir) {
                d += step;
            }
            if d < 36.0 {
                continue;
            }
            let far = start + dir * d;
            let mut pts = vec![start, start + dir * (d * 0.5), far];
            // A dog-leg to one side when there is room: the trip is not a
            // straight there and back.
            let side = rotate(dir, std::f32::consts::FRAC_PI_2 * flip);
            let off = (d * 0.35).min(120.0);
            let mut ok = true;
            let mut t = 0.0;
            while t < off {
                t += 10.0;
                if !wet(far + side * t.min(off), side) {
                    ok = false;
                    break;
                }
            }
            if ok && off >= 20.0 {
                pts.push(far + side * off);
            }
            return Some(pts);
        }
        None
    }
}

#[cfg(test)]
#[path = "waterfront_tests.rs"]
mod tests;
