//! [r7-citizen-story] Each citizen's travel history: the trips of today
//! and yesterday (when they set off, from where to where and why, the
//! modes used, the minutes it took, what happened on the way: the pound,
//! a fee paid, a parking hunt that failed), for the person pane and the
//! remote API's `inspect citizen`.
//!
//! Recording only: nothing in the simulation reads it back, and it draws
//! no random numbers. A small ring per citizen (`TRIPS_KEPT`, about forty
//! bytes a trip in memory); trips from before yesterday drop off as new
//! ones come. Saved as one compact string, deflated, as `insight` is
//! (`World::travel`, `#[serde(default)]`: older saves load with none), and
//! a string that will not decode loads empty.

use crate::buildings::BuildingId;
use crate::citizens::{CitizenId, Purpose, TripMode};
use crate::insight::{ffi, get_purpose, pack, put, put_key, put_purpose, unffi, unpack, Rd};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use slotmap::SecondaryMap;
use std::collections::VecDeque;

/// Trips kept per citizen (two ordinary days and some).
pub const TRIPS_KEPT: usize = 14;
/// Bit of `TripEntry::modes` for a drone flight (the trip modes take bits 0-7).
pub const MODE_AIR: u16 = 1 << 8;
/// Bit of `TripEntry::modes` for a seat in someone else's car.
pub const MODE_LIFT: u16 = 1 << 9;
/// Format of the saved store.
const FORMAT: u8 = 1;

/// Things that happened on a trip (`TripEntry::events`, bits).
pub mod ev {
    /// Going to get the car out of the pound.
    pub const TO_POUND: u8 = 1;
    /// Paid to get the car out of the pound (the fee in `paid`).
    pub const RELEASED: u8 = 2;
    /// Paid to park.
    pub const PAID_PARKING: u8 = 4;
    /// Lost their temper in traffic.
    pub const RAGE: u8 = 8;
    /// Waited for hours for transit that never came.
    pub const TRANSIT_FAILED: u8 = 16;
    /// Cut short: they set off somewhere else before getting there.
    pub const CUT: u8 = 32;
    /// Going to fetch the car from where it was left, far from home.
    pub const FETCH_CAR: u8 = 64;
    /// Found no space and left the car on the street.
    pub const NO_SPACE: u8 = 128;
}

/// One trip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TripEntry {
    /// Tick they set off.
    pub depart: u64,
    /// Minutes it took (once `done`, or once cut short).
    pub minutes: u16,
    pub from: Option<BuildingId>,
    pub to: Option<BuildingId>,
    pub purpose: Purpose,
    /// Modes used: bit `TripMode::index()`, `MODE_AIR`, `MODE_LIFT`.
    pub modes: u16,
    /// `ev` bits.
    pub events: u8,
    /// Money paid on the way (whole dollars): parking, tolls, the pound.
    pub paid: u16,
    pub done: bool,
}

impl TripEntry {
    pub fn mode_keys(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = TripMode::ALL.iter().filter(|m| self.modes & (1 << m.index()) != 0).map(|m| m.key()).collect();
        if self.modes & MODE_LIFT != 0 {
            out.retain(|m| *m != "car");
            out.push("lift");
        }
        if self.modes & MODE_AIR != 0 {
            out.push("air");
        }
        out
    }
}

/// Everyone's trips.
#[derive(Clone, Debug, Default)]
pub struct TravelBook {
    pub logs: SecondaryMap<CitizenId, VecDeque<TripEntry>>,
}

/// The `TripEntry::modes` bit of a mode.
pub fn mode_bit(m: TripMode) -> u16 {
    1 << m.index()
}

impl TravelBook {
    /// The trips kept for `id`, oldest first.
    pub fn trips(&self, id: CitizenId) -> impl DoubleEndedIterator<Item = &TripEntry> {
        self.logs.get(id).into_iter().flatten()
    }

    fn open(&mut self, id: CitizenId) -> Option<&mut TripEntry> {
        self.logs.get_mut(id)?.back_mut().filter(|t| !t.done)
    }

    /// They set off: a new trip (one still open is closed as cut short).
    pub fn start(&mut self, id: CitizenId, tick: u64, from: Option<BuildingId>, to: Option<BuildingId>, purpose: Purpose, modes: u16) {
        if let Some(t) = self.open(id) {
            t.done = true;
            t.events |= ev::CUT;
            t.minutes = tick.saturating_sub(t.depart).min(u16::MAX as u64) as u16;
        }
        let Some(log) = self.logs.entry(id).map(|e| e.or_default()) else { return };
        // Set off from the street (a bench, a stop): where the last trip ended.
        let from = from.or_else(|| log.back().filter(|t| t.done && t.events & ev::CUT == 0).and_then(|t| t.to));
        // Only today and yesterday are shown; older ones make room.
        let since = (tick / 1440).saturating_sub(1) * 1440;
        while log.front().is_some_and(|t| t.depart < since) || log.len() >= TRIPS_KEPT {
            log.pop_front();
        }
        log.push_back(TripEntry { depart: tick, minutes: 0, from, to, purpose, modes, events: 0, paid: 0, done: false });
    }

    /// Another mode on the trip under way (a leg after a stop, the car
    /// after the walk to it).
    pub fn add_mode(&mut self, id: CitizenId, modes: u16) {
        if let Some(t) = self.open(id) {
            t.modes |= modes;
        }
    }

    /// Something happened on the trip under way; `paid` dollars with it.
    pub fn note(&mut self, id: CitizenId, events: u8, paid: f32) {
        if let Some(t) = self.open(id) {
            t.events |= events;
            t.paid = t.paid.saturating_add(paid.max(0.0).round().min(u16::MAX as f32) as u16);
        }
    }

    /// The trip under way is over; `at` where it ended (if not where it was going).
    pub fn finish(&mut self, id: CitizenId, tick: u64, at: Option<BuildingId>) {
        if let Some(t) = self.open(id) {
            t.done = true;
            t.minutes = tick.saturating_sub(t.depart).min(u16::MAX as u64) as u16;
            if at.is_some() {
                t.to = at;
            }
        }
    }

    /// Drop the logs of people who are gone.
    pub fn retain(&mut self, alive: impl Fn(CitizenId) -> bool) {
        self.logs.retain(|k, v| alive(k) && !v.is_empty());
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = vec![FORMAT];
        for (id, log) in self.logs.iter() {
            if log.is_empty() {
                continue;
            }
            put_key(&mut b, ffi(Some(id)));
            put(&mut b, log.len() as u64);
            let mut last = 0u64;
            for t in log {
                put(&mut b, t.depart.saturating_sub(last));
                last = t.depart;
                put(&mut b, t.minutes as u64);
                put_key(&mut b, ffi(t.from));
                put_key(&mut b, ffi(t.to));
                put_purpose(&mut b, t.purpose);
                put(&mut b, t.modes as u64);
                b.push(t.events);
                put(&mut b, t.paid as u64);
                b.push(t.done as u8);
            }
        }
        b
    }

    pub fn decode(bytes: &[u8]) -> Option<TravelBook> {
        let mut r = Rd::new(bytes);
        if r.byte()? != FORMAT {
            return None;
        }
        let mut out = TravelBook::default();
        while !r.done() {
            let id: CitizenId = unffi(r.key()?)?;
            let n = r.u()? as usize;
            let mut log = VecDeque::with_capacity(n.min(TRIPS_KEPT));
            let mut last = 0u64;
            for _ in 0..n {
                let depart = last + r.u()?;
                last = depart;
                let minutes = r.u()?.min(u16::MAX as u64) as u16;
                let from = unffi(r.key()?);
                let to = unffi(r.key()?);
                let purpose = get_purpose(&mut r)?;
                let modes = r.u()? as u16;
                let events = r.byte()?;
                let paid = r.u()?.min(u16::MAX as u64) as u16;
                let done = r.byte()? != 0;
                log.push_back(TripEntry { depart, minutes, from, to, purpose, modes, events, paid, done });
            }
            out.logs.insert(id, log);
        }
        Some(out)
    }
}

impl Serialize for TravelBook {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&pack(&self.encode()))
    }
}

impl<'de> Deserialize<'de> for TravelBook {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(unpack(&s).and_then(|b| TravelBook::decode(&b)).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn ids() -> (CitizenId, BuildingId, BuildingId) {
        let mut cs: SlotMap<CitizenId, ()> = SlotMap::with_key();
        let mut bs: SlotMap<BuildingId, ()> = SlotMap::with_key();
        (cs.insert(()), bs.insert(()), bs.insert(()))
    }

    #[test]
    fn the_ring_keeps_today_and_yesterday_and_at_most_trips_kept() {
        let (c, a, b) = ids();
        let mut book = TravelBook::default();
        // Day 0 (two days before day 2): dropped once day 2's trips come.
        book.start(c, 100, Some(a), Some(b), Purpose::Work, mode_bit(TripMode::Walk));
        book.finish(c, 130, None);
        book.start(c, 1440 + 480, Some(b), Some(a), Purpose::Home, mode_bit(TripMode::Bike));
        book.finish(c, 1440 + 500, None);
        book.start(c, 2 * 1440 + 480, Some(a), Some(b), Purpose::Work, mode_bit(TripMode::Car));
        let left: Vec<u64> = book.trips(c).map(|t| t.depart).collect();
        assert_eq!(left, vec![1440 + 480, 2 * 1440 + 480], "only yesterday's and today's are kept");
        for k in 0..40 {
            book.start(c, 2 * 1440 + 600 + k, Some(a), Some(b), Purpose::Wander, mode_bit(TripMode::Walk));
        }
        assert_eq!(book.trips(c).count(), TRIPS_KEPT);
        // Every trip set off before the next was cut short.
        assert!(book.trips(c).rev().skip(1).all(|t| t.done && t.events & ev::CUT != 0));
    }

    #[test]
    fn modes_events_and_minutes_are_kept_and_the_book_round_trips() {
        let (c, a, b) = ids();
        let mut book = TravelBook::default();
        book.start(c, 7 * 1440 + 500, Some(a), Some(b), Purpose::Wander, mode_bit(TripMode::Bike));
        book.note(c, ev::TO_POUND, 0.0);
        book.finish(c, 7 * 1440 + 640, None);
        book.start(c, 7 * 1440 + 640, Some(b), Some(a), Purpose::Home, mode_bit(TripMode::Car));
        book.note(c, ev::RELEASED, 95.0);
        book.add_mode(c, mode_bit(TripMode::Walk));
        book.finish(c, 7 * 1440 + 682, None);
        let trips: Vec<TripEntry> = book.trips(c).copied().collect();
        assert_eq!(trips[0].minutes, 140);
        assert_eq!(trips[0].mode_keys(), vec!["bike"]);
        assert_eq!(trips[1].paid, 95);
        assert_eq!(trips[1].mode_keys(), vec!["walk", "car"]);
        assert!(trips[1].events & ev::RELEASED != 0 && trips[1].done);
        // Saved and loaded: the same.
        let text = ron::to_string(&book).unwrap();
        let back: TravelBook = ron::from_str(&text).unwrap();
        assert_eq!(back.trips(c).copied().collect::<Vec<_>>(), trips);
        // A store that will not decode loads empty.
        let junk: TravelBook = ron::from_str("\"z!!!\"").unwrap();
        assert_eq!(junk.logs.len(), 0);
    }
}
