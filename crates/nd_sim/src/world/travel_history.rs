//! [r7-citizen-story] Recording each citizen's trips (`crate::travel_log`)
//! where they are decided and where they end, and reading them back in
//! words for the person pane, `inspect citizen` and the minds' prompts:
//! the trips of today and yesterday, and where the car is now.
//!
//! Recording only: nothing here feeds back into the simulation.

use super::*;
use crate::enforcement::STORAGE_PER_DAY;
use crate::travel_log::{ev, TripEntry};
use crate::vehicle::{CarPlace, ParkKind};

/// A trip as shown: everything in words, the places as buildings too.
#[derive(Clone, Debug, PartialEq)]
pub struct TripView {
    pub depart: u64,
    /// Minutes it took; `None` while under way.
    pub minutes: Option<u32>,
    pub from: Option<BuildingId>,
    pub from_label: String,
    pub to: Option<BuildingId>,
    pub to_label: String,
    /// "home", "to work", "to get the car out of the pound".
    pub why: String,
    /// Mode keys ("walk", "bike", "car", "bus", "lift", "air").
    pub modes: Vec<&'static str>,
    /// What happened on the way, in words.
    pub events: Vec<String>,
    pub cut_short: bool,
}

impl TripView {
    /// One line: "08:47-11:10 Ember Terrace → Hooked on Towing, to get the
    /// car out of the pound · bike · 143 min".
    pub fn line(&self) -> String {
        let when = format!("d{} {:02}:{:02}", self.depart / 1440 + 1, (self.depart % 1440) / 60, self.depart % 60);
        let took = match self.minutes {
            Some(m) if self.cut_short => format!("{m} min, then went elsewhere"),
            Some(m) => format!("{m} min"),
            None => "under way".to_string(),
        };
        let mut s = format!("{when}  {} → {}, {}  ·  {}  ·  {took}", self.from_label, self.to_label, self.why, mode_words(&self.modes));
        for e in &self.events {
            s.push_str("  ·  ");
            s.push_str(e);
        }
        s
    }
}

/// Mode keys in words: "bike", "walk + car", "bus".
pub fn mode_words(modes: &[&str]) -> String {
    if modes.is_empty() {
        return "on foot".into();
    }
    modes
        .iter()
        .map(|m| match *m {
            "walk" => "walk",
            "subway" => "metro",
            "lift" => "a lift",
            "air" => "drone",
            m => m,
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn why_words(t: &TripEntry) -> String {
    if t.events & ev::TO_POUND != 0 {
        return "to get the car out of the pound".into();
    }
    if t.events & ev::FETCH_CAR != 0 {
        return "to fetch the car".into();
    }
    match t.purpose {
        Purpose::Home => "home".into(),
        Purpose::Work => "to work".into(),
        Purpose::Visit(LandUse::Park) => "to the park".into(),
        Purpose::Visit(lu) => format!("out ({})", lu.label().to_lowercase()),
        Purpose::Shop(r) => format!("shopping for {}", r.label()),
        Purpose::Wander => "a wander".into(),
        Purpose::Sit => "to sit on a bench".into(),
        Purpose::ToStop => "to a stop".into(),
    }
}

impl World {
    /// [r7-citizen-story] A trip (`fresh`) or a leg of one set off by
    /// `modes` (`travel_log::mode_bit`) from `origin` to `target`.
    pub(in crate::world) fn log_leg(&mut self, id: CitizenId, origin: Option<BuildingId>, target: BuildingId, purpose: Purpose, modes: u16, fresh: bool) {
        if fresh {
            // Not inside anything (at home but stepped out, on a bench):
            // where the last trip ended (`TravelBook::start`).
            let from = origin.or_else(|| self.citizens.get(id).and_then(|c| c.inside));
            self.travel.start(id, self.clock.tick, from, Some(target), purpose, modes);
        } else {
            self.travel.add_mode(id, modes);
        }
    }

    /// [r7-citizen-story] The trip is over at `at`: what it cost on the way
    /// (parking, tolls), a temper lost, and the minutes.
    pub(in crate::world) fn log_arrived(&mut self, id: CitizenId, at: Option<BuildingId>) {
        let Some(c) = self.citizens.get(id) else { return };
        let mut events = 0;
        if c.commute.fee > 0.0 {
            events |= ev::PAID_PARKING;
        }
        if c.commute.rage >= 0.5 {
            events |= ev::RAGE;
        }
        let paid = c.commute.fee + c.commute.toll;
        self.travel.note(id, events, paid);
        self.travel.finish(id, self.clock.tick, at);
    }

    /// [r7-citizen-story] The trips of the last `days` days (1: today
    /// only), oldest first, in words.
    pub fn travel_history(&self, id: CitizenId, days: u64) -> Vec<TripView> {
        let since = (self.clock.day() + 1).saturating_sub(days.max(1)) * 1440;
        let name = |b: Option<BuildingId>, fallback: &str| b.and_then(|b| self.buildings.get(b)).map_or_else(|| fallback.to_string(), |b| b.name.clone());
        let c = self.citizens.get(id);
        let home = c.and_then(|c| c.home);
        let work = c.and_then(|c| c.work);
        let label = |b: Option<BuildingId>, fallback: &str| {
            let n = name(b, fallback);
            if b.is_some() && b == home {
                format!("home ({n})")
            } else if b.is_some() && b == work {
                format!("work ({n})")
            } else {
                n
            }
        };
        let traveling = c.is_some_and(|c| c.activity.is_traveling() || c.to_car.is_some() || c.transit.is_some() || matches!(c.activity, Activity::Passenger { .. }));
        let trips: Vec<&TripEntry> = self.travel.trips(id).collect();
        let n = trips.len();
        trips
            .into_iter()
            .enumerate()
            .filter(|(_, t)| t.depart >= since)
            .map(|(i, t)| {
                let last = i + 1 == n;
                let open = !t.done;
                let mut events = Vec::new();
                if t.events & ev::RELEASED != 0 {
                    events.push(format!("paid {} to get the car out of the pound", t.paid));
                } else if t.paid > 0 {
                    events.push(format!("paid {}{}", t.paid, if t.events & ev::PAID_PARKING != 0 { " to park" } else { " in tolls" }));
                }
                if t.events & ev::NO_SPACE != 0 {
                    events.push("no space, left the car on the street".into());
                }
                if t.events & ev::RAGE != 0 {
                    events.push("lost their temper in traffic".into());
                }
                if t.events & ev::TRANSIT_FAILED != 0 {
                    events.push("waited hours for transit that never came".into());
                }
                // An open trip that is not the one under way never ended
                // where it was going (a ride that fell through, a plan dropped).
                let dropped = open && !(last && traveling);
                let minutes = if t.done || dropped { Some(if t.done { t.minutes as u32 } else { self.clock.tick.saturating_sub(t.depart) as u32 }) } else { None };
                TripView {
                    depart: t.depart,
                    minutes: if dropped { None } else { minutes },
                    from: t.from,
                    from_label: label(t.from, "the street"),
                    to: t.to,
                    to_label: label(t.to, "somewhere"),
                    why: why_words(t),
                    modes: t.mode_keys(),
                    events: {
                        if dropped {
                            events.push("never got there".into());
                        }
                        events
                    },
                    cut_short: t.events & ev::CUT != 0,
                }
            })
            .collect()
    }

    /// [r7-citizen-story] The trips of the last `days` days as lines.
    pub fn travel_lines(&self, id: CitizenId, days: u64) -> Vec<String> {
        self.travel_history(id, days).iter().map(TripView::line).collect()
    }

    /// [r7-citizen-story] Where the citizen's car is, in words (`None`
    /// without one): "in the pound at Hooked on Towing: 95 to get it out,
    /// 15 more each day it stays", "parked at the kerb 65 m from home, a
    /// 2-hour limit (ticketed)", "parked off the street at Lotus Greenway".
    pub fn car_whereabouts(&self, id: CitizenId) -> Option<String> {
        let c = self.citizens.get(id)?;
        let vid = c.car?;
        let car = self.vehicles.cars.get(vid)?;
        let name = |b: Option<BuildingId>| b.and_then(|b| self.buildings.get(b)).map(|b| b.name.clone());
        let spot = match car.place {
            CarPlace::Driving(_) => return Some(if matches!(c.activity, Activity::Driving { .. }) { "being driven by them right now".into() } else { "on the road".into() }),
            CarPlace::Parked(s) => s,
        };
        let home = c.home.and_then(|h| self.building_entrance(h));
        let from_home = home.map(|h| h.distance(spot.pos));
        let near = |d: Option<f32>| d.map_or(String::new(), |d| if d < 1000.0 { format!(" {:.0} m from home", d) } else { format!(" {:.1} km from home", d / 1000.0) });
        Some(match spot.kind {
            ParkKind::Pound => {
                let at = name(spot.building).unwrap_or_else(|| "a pound out of town".into());
                format!("in the pound at {at}: {:.0} to get it out now, {STORAGE_PER_DAY:.0} more each day it stays", self.release_fee(&spot))
            }
            ParkKind::Hooked => "on a tow truck, on its way to the pound".into(),
            ParkKind::Dealer => "on a dealer's forecourt".into(),
            ParkKind::Home => format!("parked in a space at home ({})", name(spot.building).unwrap_or_default()),
            ParkKind::Building => format!("parked off the street at {}", name(spot.building).unwrap_or_else(|| "a building".into())),
            ParkKind::Lot => format!("in the car park at {}", name(spot.building).unwrap_or_else(|| "a car park".into())),
            ParkKind::Kerb | ParkKind::Overflow => {
                let mut s = if spot.kind == ParkKind::Overflow { format!("left on the street where parking is banned,{}", near(from_home)) } else { format!("parked at the kerb{}", near(from_home)) };
                if spot.kind == ParkKind::Kerb {
                    match self.kerb_limit(vid) {
                        None => s.push_str(" (residents' permit, no limit)"),
                        Some(l) => s.push_str(&format!(", a {}-hour limit", l / 60)),
                    }
                }
                let hours = self.clock.tick.saturating_sub(spot.since) / 60;
                if hours >= 2 {
                    s.push_str(&format!(", there {hours} h"));
                }
                if spot.booted {
                    s.push_str(", clamped");
                } else if spot.ticketed > 0 {
                    s.push_str(", ticketed");
                }
                s
            }
        })
    }
}
