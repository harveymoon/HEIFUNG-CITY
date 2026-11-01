//! [sound] The spot sounds' end in the game: four times a second, what
//! around the camera could make a noise (trains, metro trains, units on a
//! call, building sites at work, open clubs, bars, karaoke boxes and
//! concert halls, cars stuck and fuming in a jam, a storm), read from the
//! sim (never written), handed to `nd_audio::spots::pick` and the horn
//! and thunder clocks, and sent to the engine.

use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::prelude::*;
use nd_audio::scene::height_weights;
use nd_audio::spots::{pick, Candidate, HornClock, Listener, SpotKind, ThunderClock};
use nd_audio::{Command, SpotUpdate};
use nd_sim::Vec2 as SVec2;

use super::Sound;
use crate::camera::{CameraMode, OrbitCamera};
use crate::render::Lighting;
use crate::sim::Sim;
use crate::tools::{Depth, ToolState};

/// Seconds between samples.
pub const EVERY: f32 = 0.25;
/// Nothing further than this (metres, flat) from the spot looked at is
/// considered: the furthest-heard kind (a siren) is silent past it.
const REACH: f32 = 1_700.0;
/// Stuck this long (seconds) and angry this much, a driver may honk.
const HORN_STALL: f32 = 12.0;
const HORN_ANGER: f32 = 0.25;

/// The spot sounds' state between samples.
#[derive(Default)]
pub(super) struct SpotState {
    next: f64,
    horns: HornClock,
    thunder: ThunderClock,
}

/// What the spot sounds are doing, for the remote readout.
#[derive(Clone, Debug, Default)]
pub struct SpotReport {
    /// The held sounds picked last, loudest first.
    pub held: Vec<SpotUpdate>,
    /// Candidates of each kind considered.
    pub candidates: [u32; SpotKind::ALL.len()],
    /// Horns and thunderclaps sent since the start.
    pub horns: u64,
    pub thunder: u64,
    /// How stormy it is (`0..1`) and the echo sent.
    pub storm: f32,
    pub room: f32,
    /// The last sample's cost on the main thread, microseconds.
    pub cost_us: f32,
}

fn id_of(tag: u64, key: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    tag.hash(&mut h);
    key.hash(&mut h);
    h.finish()
}

/// The velocity of something heading `h` (radians, sim axes) at `v` m/s.
fn vel(h: f32, v: f32) -> [f32; 3] {
    [h.cos() * v, 0.0, h.sin() * v]
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spots(
    mut sound: ResMut<Sound>,
    mut state: Local<SpotState>,
    time: Res<Time<Real>>,
    sim: Res<Sim>,
    lighting: Res<Lighting>,
    tools: Res<ToolState>,
    mode: Res<CameraMode>,
    eyes: Query<(&GlobalTransform, &OrbitCamera)>,
) {
    let now = time.elapsed_secs_f64();
    if now < state.next {
        return;
    }
    state.next = now + EVERY as f64;
    let Some((gt, orbit)) = eyes.iter().next() else { return };
    let start = std::time::Instant::now();
    let w = &sim.world;
    let eye = gt.translation();
    let focus = if *mode == CameraMode::Walk { SVec2::new(eye.x, eye.z) } else { SVec2::new(orbit.target.x, orbit.target.z) };
    let ground = w.surface_height(focus).max(nd_sim::landscape::WATER_LEVEL);
    let right = gt.right();
    let underground = tools.depth == Depth::Underground;
    let listener = Listener { eye: eye.to_array(), focus: [focus.x, ground, focus.y], right: right.to_array(), height: sound.heard.height, underground };
    let groups = super::settings().group_gains();
    let paused = sim.paused;
    let (cands, jams) = candidates(w, focus, paused);
    let mut report = SpotReport { horns: sound.spots.horns, thunder: sound.spots.thunder, ..Default::default() };
    for c in cands.iter().chain(jams.iter()) {
        report.candidates[c.kind as usize] += 1;
    }
    let held = pick(&cands, &listener, &groups);
    // The buildings' echo: a dense street rings, open country and the view from up high do not.
    let [_, mid, street] = height_weights(sound.heard.height);
    let room = sound.heard.density * (street + 0.4 * mid);
    sound.handle.send(Command::Room(room));
    for u in &held {
        sound.handle.send(Command::Spot(*u));
    }
    if !paused {
        for u in state.horns.tick(&jams, &listener, &groups, EVERY) {
            report.horns += 1;
            sound.handle.send(Command::Spot(u));
        }
        let storm = storm(w, &lighting);
        report.storm = storm;
        for (_, u) in state.thunder.tick(storm, &groups, underground, EVERY) {
            report.thunder += 1;
            sound.handle.send(Command::Spot(u));
        }
    }
    report.held = held;
    report.room = room;
    report.cost_us = start.elapsed().as_secs_f32() * 1e6;
    sound.spots = report;
}

/// How stormy it is, `0..1`: a storm spell with its rain, or a heavy
/// downpour now and then.
pub fn storm(w: &nd_sim::World, lighting: &Lighting) -> f32 {
    let rain = crate::render::climate::precip_shown(w, lighting).filter(|p| p.0 == "rain").map_or(0.0, |p| p.1);
    if w.weather.spell == nd_sim::weather::Spell::Storm {
        rain.max(0.6)
    } else if rain > 0.75 {
        0.35 * rain
    } else {
        0.0
    }
}

/// The held candidates near `focus` and the jams (horn candidates).
pub fn candidates(w: &nd_sim::World, focus: SVec2, paused: bool) -> (Vec<Candidate>, Vec<Candidate>) {
    let reach2 = REACH * REACH;
    let near = |p: SVec2| p.distance_squared(focus) < reach2;
    let ground = |p: SVec2| if w.relief { w.ground_height(p) } else { 0.0 };
    let mut out = Vec::new();
    // Trains: the middle of the train, at its speed now.
    for (id, t) in w.rail.trains.iter() {
        let (p, h) = t.vehicle_pose_at((t.cars as usize).div_ceil(2), t.s);
        if !near(p) {
            continue;
        }
        let v = if paused { 0.0 } else { t.v };
        out.push(Candidate { vel: vel(h, v), ..Candidate::at(id_of(1, id), SpotKind::Train, p.x, ground(p) + 1.0, p.y) });
    }
    for (id, t) in w.subway.trains.iter() {
        let (p, h) = t.car_pose_at(nd_sim::subway::CARS as usize / 2, t.run.s);
        if !near(p) {
            continue;
        }
        let v = if paused { 0.0 } else { t.run.v };
        let c = Candidate::at(id_of(2, id), SpotKind::Metro, p.x, ground(p) - nd_sim::subway::DEPTH, p.y);
        out.push(Candidate { vel: vel(h, v), ..c });
    }
    // Units on a call, driving: the siren. The id carries the kind (id % 3).
    for (uid, u) in w.services.units.iter() {
        let Some(car) = u.car.filter(|_| u.lights()).and_then(|c| w.traffic.cars.get(c)) else { continue };
        if !near(car.pos) {
            continue;
        }
        let variant = match u.kind {
            nd_sim::services::UnitKind::PoliceCar => 0,
            nd_sim::services::UnitKind::Ambulance => 1,
            nd_sim::services::UnitKind::FireEngine => 2,
        };
        let h = id_of(3, uid);
        let id = h - h % 3 + variant;
        let v = if paused { 0.0 } else { car.v };
        out.push(Candidate { vel: vel(car.heading, v), ..Candidate::at(id, SpotKind::Siren, car.pos.x, ground(car.pos) + 1.0, car.pos.y) });
    }
    // Building sites, while the crews are at work.
    if w.clock.is_work_hours() {
        for (bid, _, _) in w.construction_sites() {
            let Some(b) = w.buildings.get(bid) else { continue };
            if b.construction.as_ref().is_some_and(|c| c.blocked.is_some()) {
                continue;
            }
            let Some(lot) = w.lots.get(b.lot) else { continue };
            if !near(lot.center) {
                continue;
            }
            let c = Candidate::at(id_of(4, bid), SpotKind::Works, lot.center.x, ground(lot.center) + 3.0, lot.center.y);
            out.push(Candidate { intensity: if paused { 0.4 } else { 1.0 }, ..c });
        }
    }
    // The night's venues, while open: louder the fuller.
    let hour = w.clock.hour() as u8;
    for (bid, b) in w.buildings.iter() {
        let Some(z) = b.business.as_ref() else { continue };
        use nd_sim::BusinessKind as K;
        let kind = match z.kind {
            K::Venue => SpotKind::Club,
            K::Bar | K::BeachBar => SpotKind::Bar,
            K::KaraokeBox => SpotKind::Karaoke,
            K::ConcertHall => SpotKind::MusicHall,
            _ => continue,
        };
        if z.closed || !b.operating() || !b.opening_hours().contains(hour) {
            continue;
        }
        let Some(lot) = w.lots.get(b.lot) else { continue };
        if !near(lot.center) {
            continue;
        }
        let full = b.visitors as f32 / b.customer_capacity().max(1) as f32;
        let c = Candidate::at(id_of(5, bid), kind, lot.center.x, ground(lot.center) + 2.0, lot.center.y);
        out.push(Candidate { intensity: (0.35 + 0.65 * full).min(1.0), ..c });
    }
    // Jams: cars stood still a while behind others, their drivers fuming.
    let mut jams = Vec::new();
    for (cid, car) in w.traffic.cars.iter() {
        if car.v > 0.5 || car.stalled < HORN_STALL || car.drive.anger < HORN_ANGER || car.siren || !near(car.pos) {
            continue;
        }
        use nd_sim::traffic::Wait;
        if !matches!(car.wait, Wait::Leader | Wait::ExitFull | Wait::Blocked | Wait::Yield) {
            continue;
        }
        let c = Candidate::at(id_of(6, cid), SpotKind::Horn, car.pos.x, ground(car.pos) + 1.0, car.pos.y);
        jams.push(Candidate { intensity: car.drive.anger.clamp(0.0, 1.0), ..c });
    }
    (out, jams)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_city_is_quiet_and_ids_are_stable() {
        let w = nd_sim::World::new(nd_sim::WorldConfig { emergencies: false, society: false, landscape: nd_sim::LandscapeParams::flat(), ..Default::default() });
        let (held, jams) = candidates(&w, SVec2::ZERO, false);
        assert!(held.is_empty() && jams.is_empty());
        assert_eq!(id_of(1, 42u32), id_of(1, 42u32));
        assert_ne!(id_of(1, 42u32), id_of(2, 42u32));
        // Heading 0 is +x in the sim, which is +x in the game's 3D.
        let v = vel(0.0, 10.0);
        assert!((v[0] - 10.0).abs() < 1e-5 && v[2].abs() < 1e-5);
        let v = vel(std::f32::consts::FRAC_PI_2, 10.0);
        assert!((v[2] - 10.0).abs() < 1e-4);
    }
}
