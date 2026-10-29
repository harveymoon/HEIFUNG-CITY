//! [airlift] The emergency airlift: a twin-rotor heavy-lift helicopter
//! (a Chinook) flies in from the map edge, hovers over a vehicle stuck on
//! the road, hooks it on a sling and flies it away: a private car to the
//! pound (the nearest tow yard, else one out of town), anything else off
//! the map. The player calls it from the inspector on any vehicle on the
//! road (`World::airlift`); it costs `AIRLIFT_COST` from the treasury.
//!
//! The vehicle stays where it stands until the helicopter is over it
//! (`AIRLIFT_INBOUND` ticks), is lifted in `AIRLIFT_HOOK` and is gone from
//! the road from then on; the flight out takes `AIRLIFT_OUTBOUND`. Where
//! the helicopter is at any moment is `Airlift::pose` (drawn by the app).

use super::*;
use crate::{Errand, VehicleId};
use serde::{Deserialize, Serialize};

/// What an airlift costs the city.
pub const AIRLIFT_COST: f32 = 4000.0;
/// Ticks flying in, hovering to hook on, and flying out.
pub const AIRLIFT_INBOUND: u64 = 8;
pub const AIRLIFT_HOOK: u64 = 4;
pub const AIRLIFT_OUTBOUND: u64 = 10;
/// Cruise height and the hover over the vehicle (m above it).
pub const AIRLIFT_CRUISE: f32 = 70.0;
pub const AIRLIFT_HOVER: f32 = 16.0;
/// How far off the map the helicopter comes from and goes to.
const OFF_MAP: f32 = 200.0;

/// One airlift under way.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Airlift {
    /// The vehicle, while it is still on the road (None once lifted).
    pub car: Option<CarId>,
    /// What it looks like, for the sling.
    pub model: crate::vehicle::VehicleModel,
    /// Where it stood (the hover point), and its heading there.
    pub at: Vec2,
    pub heading: f32,
    /// Where the helicopter comes from and goes to.
    pub from: Vec2,
    pub to: Vec2,
    /// The pound it takes a private car to (None: off the map).
    pub pound: Option<BuildingId>,
    pub started: u64,
}

/// Where an airlift's helicopter is: position (x, height, y), heading,
/// and whether the vehicle hangs under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirliftPose {
    pub pos: Vec2,
    pub height: f32,
    pub heading: f32,
    pub carrying: bool,
    /// Done: it is off the map, or at the pound.
    pub done: bool,
}

impl Airlift {
    pub fn hook_tick(&self) -> u64 {
        self.started + AIRLIFT_INBOUND
    }

    pub fn end_tick(&self) -> u64 {
        self.started + AIRLIFT_INBOUND + AIRLIFT_HOOK + AIRLIFT_OUTBOUND
    }

    /// The helicopter at `tick` plus `frac` of the next (`ground`: the
    /// height of the ground under the vehicle).
    pub fn pose(&self, tick: u64, frac: f32, ground: f32) -> AirliftPose {
        let t = tick.saturating_sub(self.started) as f32 + frac.clamp(0.0, 1.0);
        let (inb, hook, out) = (AIRLIFT_INBOUND as f32, AIRLIFT_HOOK as f32, AIRLIFT_OUTBOUND as f32);
        let ease = |x: f32| {
            let x = x.clamp(0.0, 1.0);
            x * x * (3.0 - 2.0 * x)
        };
        let face = |a: Vec2, b: Vec2| {
            let d = b - a;
            if d.length_squared() < 1e-6 { self.heading } else { d.y.atan2(d.x) }
        };
        if t < inb {
            // In: cruising, then slowing and coming down over the vehicle.
            let x = ease(t / inb);
            let h = ground + AIRLIFT_HOVER + (AIRLIFT_CRUISE - AIRLIFT_HOVER) * (1.0 - ease((t / inb - 0.6) / 0.4));
            AirliftPose { pos: self.from.lerp(self.at, x), height: h, heading: face(self.from, self.at), carrying: false, done: false }
        } else if t < inb + hook {
            // Over it: down on the sling's length, hooked at half time, up.
            let x = (t - inb) / hook;
            let dip = (x * std::f32::consts::PI).sin() * 4.0;
            AirliftPose { pos: self.at, height: ground + AIRLIFT_HOVER - dip, heading: face(self.from, self.at), carrying: x >= 0.5, done: false }
        } else if t < inb + hook + out {
            let x = ease((t - inb - hook) / out);
            let h = ground + AIRLIFT_HOVER + (AIRLIFT_CRUISE - AIRLIFT_HOVER) * ease((t - inb - hook) / out * 2.5);
            AirliftPose { pos: self.at.lerp(self.to, x), height: h, heading: face(self.at, self.to), carrying: true, done: false }
        } else {
            AirliftPose { pos: self.to, height: ground + AIRLIFT_CRUISE, heading: face(self.at, self.to), carrying: true, done: true }
        }
    }
}

/// The point off the map edge nearest `p` (square map of half size `half`).
fn off_map_near(p: Vec2, half: f32) -> Vec2 {
    let dx = half - p.x.abs();
    let dy = half - p.y.abs();
    if dx < dy {
        Vec2::new(p.x.signum() * (half + OFF_MAP), p.y)
    } else {
        Vec2::new(p.x, p.y.signum() * (half + OFF_MAP))
    }
}

impl World {
    /// [airlift] Call the airlift for vehicle `car` on the road. Refused
    /// for a bus (its riders), a unit on a call, or one already being
    /// lifted, and when the treasury cannot pay.
    pub fn airlift(&mut self, car: CarId) -> Result<(), String> {
        let c = self.traffic.cars.get(car).ok_or("no such vehicle on the road")?;
        if c.bus.is_some() {
            return Err("a bus full of people is not slung under a helicopter: clear the road round it instead".into());
        }
        if c.siren {
            return Err("it is on an emergency call".into());
        }
        if self.airlifts.iter().any(|a| a.car == Some(car)) {
            return Err("the airlift is already on its way for it".into());
        }
        if self.economy.treasury < AIRLIFT_COST {
            return Err(format!("the treasury has {:.0}; an airlift costs {AIRLIFT_COST:.0}", self.economy.treasury.max(0.0)));
        }
        let (at, heading, model, owned) = (c.pos, c.heading, c.model, c.owned);
        self.economy.debit(Flow::ServiceUpkeep, AIRLIFT_COST);
        let half = self.landscape.land_half() - 40.0;
        let from = off_map_near(at, half);
        // A resident's car goes to the pound; anything else off the map.
        let pound = owned.filter(|v| self.vehicles.cars.contains_key(*v)).and_then(|_| self.tow_company_near(at));
        let to = match pound.and_then(|b| self.building_entrance(b)) {
            Some(p) => p,
            None => off_map_near(at + (at - from).normalize_or_zero() * 10.0, half),
        };
        let tick = self.clock.tick;
        self.airlifts.push(Airlift { car: Some(car), model, at, heading, from, to, pound, started: tick });
        Ok(())
    }

    /// [airlift] Lift the vehicles the helicopters are over; done ones go.
    pub(super) fn step_airlifts(&mut self) {
        if self.airlifts.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        let due: Vec<(usize, CarId, Option<BuildingId>)> = self.airlifts.iter().enumerate().filter(|(_, a)| tick >= a.hook_tick() + AIRLIFT_HOOK / 2).filter_map(|(k, a)| a.car.map(|c| (k, c, a.pound))).collect();
        for (k, car, pound) in due {
            if let Some(c) = self.traffic.cars.get(car) {
                // Lifted from where it stands now.
                self.airlifts[k].at = c.pos;
            }
            self.airlifts[k].car = None;
            self.lift_off_road(car, pound);
        }
        self.airlifts.retain(|a| tick < a.end_tick());
    }

    /// [airlift] Take `car` off the road for good, settling what it was
    /// doing: a tow truck's job, a wreck, a unit, a resident's car (to the
    /// `pound`), a lorry's load, an away fan, whoever was in it.
    pub(in crate::world) fn lift_off_road(&mut self, car: CarId, pound: Option<BuildingId>) {
        let Some(c) = self.traffic.cars.get(car) else { return };
        let (errand, owned, pos, cargo) = (c.errand, c.owned, c.pos, c.cargo);
        let mut people: Vec<CitizenId> = c.occupants.clone();
        people.push(c.citizen);
        let tick = self.clock.tick;
        // A tow truck: its towed car to the pound, its job done or open again.
        if let Errand::Tow(job) = errand {
            if let Some(j) = self.enforcement.job(job).copied() {
                if j.state == crate::enforcement::TowState::ToPound {
                    self.impound_airlifted(j.car, j.company);
                    self.enforcement.jobs.retain(|x| x.id != job);
                } else if let Some(x) = self.enforcement.jobs.iter_mut().find(|x| x.id == job) {
                    x.truck = None;
                    x.state = crate::enforcement::TowState::Waiting { until: tick + 10 };
                }
            }
        }
        // A recovery truck: the wreck waits for another (or has gone with it).
        for w in self.driving.wrecks.iter_mut().filter(|w| w.truck == Some(car)) {
            w.truck = None;
        }
        if self.driving.tows.iter().any(|t| t.truck == car) {
            self.driving.tows.retain(|t| t.truck != car);
            self.driving.towed_total += 1;
        }
        // A wreck itself.
        if self.driving.wrecks.iter().any(|w| w.car == car) {
            if let Some(t) = self.driving.wrecks.iter().find(|w| w.car == car).and_then(|w| w.truck) {
                if self.traffic.cars.contains_key(t) {
                    self.recovery_truck_home(t, None);
                }
            }
            self.driving.wrecks.retain(|w| w.car != car);
            self.driving.towed_total += 1;
        }
        self.driving.moves.retain(|m| m.truck != car);
        // A unit: home to its station.
        if let Some(uid) = self.services.unit_of_car(car) {
            self.pull_off_road(uid);
            self.park_at_station(uid);
            return;
        }
        // A lorry's load is delivered as if it got there.
        if let Some(cg) = cargo {
            self.deliver(cg);
        }
        self.games.driving_in.retain(|f| f.car != car);
        self.traffic.remove_car(car);
        // Whoever was in it walks on from the kerb.
        let kerb = self.kerbside(pos);
        for p in people {
            if self.citizens.get(p).is_some_and(|c| matches!(c.activity, Activity::Driving { car: x, .. } if x == car)) {
                self.walk_from_car(p, kerb);
            }
        }
        if let Some(vid) = owned.filter(|v| self.vehicles.cars.contains_key(*v)) {
            self.impound_airlifted(vid, pound);
        }
    }

    /// [airlift] A resident's car into the pound (`pound`, else one out of
    /// town), the owner told.
    fn impound_airlifted(&mut self, vid: VehicleId, pound: Option<BuildingId>) {
        self.impound(vid, pound);
        let tick = self.clock.tick;
        if let Some(owner) = self.vehicles.cars.get(vid).map(|c| c.owner) {
            if let Some(c) = self.citizens.get_mut(owner) {
                c.remember(tick, "The city airlifted my car off the road. A helicopter. Really.".to_string());
            }
        }
    }

    /// [airlift] The airlifts under way.
    pub fn airlifts(&self) -> &[Airlift] {
        &self.airlifts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helicopter_flies_in_hovers_lifts_and_flies_out() {
        let a = Airlift { car: None, model: Default::default(), at: Vec2::new(100.0, 50.0), heading: 0.0, from: Vec2::new(1400.0, 50.0), to: Vec2::new(-1400.0, 50.0), pound: None, started: 100 };
        let p0 = a.pose(100, 0.0, 0.0);
        assert!(p0.pos.distance(a.from) < 1.0 && p0.height > AIRLIFT_CRUISE - 1.0 && !p0.carrying);
        let over = a.pose(100 + AIRLIFT_INBOUND, 0.0, 0.0);
        assert!(over.pos.distance(a.at) < 0.5 && over.height <= AIRLIFT_HOVER + 0.01 && !over.carrying);
        let lifting = a.pose(100 + AIRLIFT_INBOUND + AIRLIFT_HOOK - 1, 0.0, 0.0);
        assert!(lifting.carrying && lifting.pos.distance(a.at) < 0.5);
        let away = a.pose(100 + AIRLIFT_INBOUND + AIRLIFT_HOOK + AIRLIFT_OUTBOUND / 2, 0.0, 0.0);
        assert!(away.carrying && away.pos.x < a.at.x && away.height > AIRLIFT_HOVER);
        assert!(a.pose(a.end_tick(), 0.0, 0.0).done);
        // Always continuous: no jumps between ticks.
        let mut last = a.pose(100, 0.0, 0.0);
        for k in 0..((a.end_tick() - 100) * 10) {
            let p = a.pose(100 + k / 10, (k % 10) as f32 / 10.0, 0.0);
            assert!(p.pos.distance(last.pos) < 400.0 && (p.height - last.height).abs() < 30.0, "{k}: {last:?} -> {p:?}");
            last = p;
        }
    }
}
