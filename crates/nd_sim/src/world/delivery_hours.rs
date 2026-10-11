//! [delivery-hours] Delivery lorries keep working hours.
//!
//! Orders are placed round the clock (the market books restocking on the
//! hour, building sites order their next load whenever a delivery lands),
//! and every one used to put its lorry on the road at once: at midnight a
//! dozen trucks set off for the city's building sites in the same minute.
//! Now an order is queued (`World::freight_queue`) and its lorry leaves
//! at a time of the buyer's own (`departure`):
//!
//! - in working hours (from an early start, `EARLY_START` plus up to
//!   `START_SPREAD` minutes by the buyer, until `LAST_DEPARTURE`) it goes
//!   within the hour, at a minute hashed from the buyer and the hour, so a
//!   restock round sends its lorries out one by one, not all at once;
//! - outside them it waits for the next day's round: from the buyer's
//!   early start over the following `NEXT_DAY_SPREAD` minutes (else a
//!   night's orders all came at dawn);
//! - now and then (`NIGHT_RUSH_PER_MILLE` of buyers on a given night) an
//!   order is a rush job and goes out at night anyway.
//!
//! The buyer counts the load as inbound from the order (`note_inbound`),
//! so nothing is ordered twice while it waits. Hashes, no world RNG.

use super::market::TruckOrigin;
use super::*;
use serde::{Deserialize, Serialize};

/// The earliest a delivery sets off (05:00).
pub const EARLY_START: u64 = 5 * 60;
/// Buyers' first deliveries spread over this many minutes from it.
pub const START_SPREAD: u64 = 150;
/// No lorry sets off after this (20:00) but a rush job.
pub const LAST_DEPARTURE: u64 = 20 * 60;
/// Within working hours a lorry leaves within this many minutes.
pub const HOUR_SPREAD: u64 = 50;
/// Orders held over the night go out over this many minutes from the
/// buyer's first delivery: the next day's round, not a dawn rush.
pub const NEXT_DAY_SPREAD: u64 = 6 * 60;
/// Buyers (per thousand, each night) whose order goes out as a night rush.
pub const NIGHT_RUSH_PER_MILLE: u64 = 25;

/// A lorry ordered and waiting to set off at `due`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct QueuedTruck {
    pub due: u64,
    /// The yard or business it leaves from; `None`: in from the city edge.
    pub from: Option<BuildingId>,
    pub cargo: Cargo,
    pub color: u8,
}

fn key(b: BuildingId) -> u64 {
    slotmap::Key::data(&b).as_ffi()
}

/// The minute of the day buyer `to` takes its first delivery.
pub fn first_delivery(to: BuildingId) -> u64 {
    EARLY_START + crate::hash::mix(key(to), 0xDE11_5747) % START_SPREAD
}

/// When a lorry ordered at `now` for `to` sets off (see the module notes).
pub fn departure(now: u64, to: BuildingId) -> u64 {
    let day = now / crate::clock::MINUTES_PER_DAY;
    let minute = now % crate::clock::MINUTES_PER_DAY;
    let start = first_delivery(to);
    let k = key(to);
    if (start..LAST_DEPARTURE).contains(&minute) {
        // Within the hour, at the buyer's minute of this hour.
        return now + crate::hash::mix(k ^ (now / 60), 0x5747_A66E) % HOUR_SPREAD;
    }
    // A night: the night of the evening it falls in.
    let night = if minute >= LAST_DEPARTURE { day } else { day.saturating_sub(1) };
    if crate::hash::mix(k ^ night.wrapping_mul(0x9E37), 0x4054_0001) % 1000 < NIGHT_RUSH_PER_MILLE {
        return now + crate::hash::mix(k ^ now, 0x4054_0002) % 30;
    }
    let morning = if minute >= LAST_DEPARTURE { day + 1 } else { day };
    morning * crate::clock::MINUTES_PER_DAY + start + crate::hash::mix(k ^ morning ^ now.wrapping_mul(0x2545_F491), 0x5747_0003) % NEXT_DAY_SPREAD
}

impl World {
    /// Order a lorry: it sets off at `departure`, now when that is now.
    pub(super) fn queue_truck(&mut self, origin: TruckOrigin, cargo: Cargo, color: u8) {
        let tick = self.clock.tick;
        let due = departure(tick, cargo.to);
        if due <= tick {
            self.dispatch_truck(origin, cargo, color);
            return;
        }
        let from = match origin {
            TruckOrigin::Building(b) => Some(b),
            TruckOrigin::CityEdge => None,
        };
        self.freight_queue.push(QueuedTruck { due, from, cargo, color });
    }

    /// Send the lorries whose time has come, in the order they were queued.
    /// One whose yard is gone meanwhile comes from the city edge instead;
    /// one whose buyer is gone is dropped.
    pub(super) fn step_freight_queue(&mut self) {
        let tick = self.clock.tick;
        if !self.freight_queue.iter().any(|q| q.due <= tick) {
            return;
        }
        let (go, wait): (Vec<QueuedTruck>, Vec<QueuedTruck>) = std::mem::take(&mut self.freight_queue).into_iter().partition(|q| q.due <= tick);
        self.freight_queue = wait;
        for q in go {
            if !self.buildings.contains_key(q.cargo.to) {
                continue;
            }
            let origin = match q.from.filter(|b| self.buildings.contains_key(*b)) {
                Some(b) => TruckOrigin::Building(b),
                None => TruckOrigin::CityEdge,
            };
            self.dispatch_truck(origin, q.cargo, q.color);
        }
    }

    /// [delivery-hours] Lorries waiting to set off (for the report).
    pub fn queued_trucks(&self) -> usize {
        self.freight_queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::KeyData;

    fn b(n: u64) -> BuildingId {
        BuildingId::from(KeyData::from_ffi(n | (1 << 32)))
    }

    /// Harvey: "they may start work really early but never middle of
    /// night, some extreme cases or emergency rush deliveries maybe ...
    /// trucks showing up at midnight at the same time".
    #[test]
    fn lorries_keep_working_hours_and_leave_one_by_one() {
        let day = 10 * crate::clock::MINUTES_PER_DAY;
        let buyers: Vec<BuildingId> = (1..400).map(b).collect();
        // Ordered at midnight: almost all wait for the morning, spread.
        let mut night = 0;
        let mut minutes = std::collections::BTreeSet::new();
        for &to in &buyers {
            let d = departure(day, to);
            let m = d % crate::clock::MINUTES_PER_DAY;
            if (EARLY_START..LAST_DEPARTURE).contains(&m) && d > day {
                minutes.insert(m);
            } else {
                night += 1;
                assert!(d < day + 30, "a rush goes at once");
            }
        }
        assert!(night > 0 && night < buyers.len() / 15, "a few night rushes, not most: {night}");
        assert!(minutes.len() > 100, "the morning deliveries spread over many minutes: {}", minutes.len());
        assert!(minutes.iter().all(|&m| m >= EARLY_START), "never before the early start");
        // Ordered at ten in the morning: all within the hour, many minutes.
        let ten = day + 10 * 60;
        let ds: Vec<u64> = buyers.iter().map(|&to| departure(ten, to)).collect();
        assert!(ds.iter().all(|&d| d >= ten && d < ten + HOUR_SPREAD));
        let spread: std::collections::BTreeSet<u64> = ds.iter().copied().collect();
        assert!(spread.len() >= HOUR_SPREAD as usize * 3 / 4, "one by one: {} distinct minutes", spread.len());
        // Late evening: the next morning (or a rush).
        let late = day + 22 * 60;
        for &to in &buyers {
            let d = departure(late, to);
            assert!(d < late + 30 || (d >= day + crate::clock::MINUTES_PER_DAY + EARLY_START && d < day + crate::clock::MINUTES_PER_DAY + EARLY_START + START_SPREAD + NEXT_DAY_SPREAD));
        }
    }
}
