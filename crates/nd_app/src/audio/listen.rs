//! [sound] What the camera hears: four times a second, the cars, walkers,
//! buildings, water, hour and weather around the spot under the camera,
//! read from the sim (never written) and turned into the beds' settings.

use bevy::prelude::*;
use nd_audio::{bed_params, Command, Surroundings};
use nd_sim::Vec2 as SVec2;

use super::Sound;
use crate::camera::{CameraMode, OrbitCamera};
use crate::render::Lighting;
use crate::sim::Sim;
use crate::tools::{Depth, ToolState};

/// Seconds between samples.
const EVERY: f64 = 0.25;
/// "Close": the street bed's radius, metres.
const CLOSE: f32 = 60.0;

/// The radius counted around the listener at a height: the street's width
/// low down, a district from up high.
pub fn radius_for(height: f32) -> f32 {
    (height * 1.2).clamp(CLOSE, 1_500.0)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn listen(
    mut sound: ResMut<Sound>,
    mut next: Local<f64>,
    time: Res<Time<Real>>,
    sim: Res<Sim>,
    lighting: Res<Lighting>,
    tools: Res<ToolState>,
    mode: Res<CameraMode>,
    eyes: Query<(&GlobalTransform, &OrbitCamera)>,
) {
    let now = time.elapsed_secs_f64();
    // [sound] Sampled even when silent (ND_AUDIO=0) so the remote readout
    // shows what would be heard; four times a second, cheap.
    if now < *next {
        return;
    }
    *next = now + EVERY;
    let Some((eye, orbit)) = eyes.iter().next() else { return };
    let eye = eye.translation();
    let focus = if mode.street() { SVec2::new(eye.x, eye.z) } else { SVec2::new(orbit.target.x, orbit.target.z) };
    let heard = surroundings(&sim.world, &lighting, tools.depth == Depth::Underground, eye, focus);
    let beds = bed_params(&heard);
    sound.heard = heard;
    sound.beds = beds;
    sound.handle.send(Command::Beds(beds));
}

/// The surroundings of `focus` heard from `eye`.
pub fn surroundings(w: &nd_sim::World, lighting: &Lighting, underground: bool, eye: Vec3, focus: SVec2) -> Surroundings {
    let ground = w.surface_height(SVec2::new(eye.x, eye.z)).max(nd_sim::landscape::WATER_LEVEL);
    let height = (eye.y - ground).max(0.5);
    let radius = radius_for(height);
    let (r2, c2) = (radius * radius, CLOSE * CLOSE);
    // Moving cars and their speed.
    let (mut cars_close, mut cars_near, mut speed_sum) = (0.0f32, 0.0f32, 0.0f32);
    for (_, car) in w.traffic.cars.iter() {
        if car.v < 0.5 {
            continue;
        }
        let d2 = car.pos.distance_squared(focus);
        if d2 < r2 {
            cars_near += 1.0;
            speed_sum += car.v;
            if d2 < c2 {
                cars_close += 1.0;
            }
        }
    }
    // People out on foot.
    let (mut walkers_close, mut walkers_near) = (0.0f32, 0.0f32);
    for (_, c) in w.citizens.iter() {
        if !c.is_visible() {
            continue;
        }
        let d2 = c.pos.distance_squared(focus);
        if d2 < r2 {
            walkers_near += 1.0;
            if d2 < c2 {
                walkers_close += 1.0;
            }
        }
    }
    // The buildings: how built up, how neon, how green.
    let br = radius.clamp(80.0, 600.0);
    let b2 = br * br;
    let (mut floors, mut n, mut neon, mut green, mut industry) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for (_, b) in w.buildings.iter() {
        let Some(lot) = w.lots.get(b.lot) else { continue };
        if lot.center.distance_squared(focus) >= b2 {
            continue;
        }
        n += 1.0;
        floors += b.floors.max(1) as f32;
        let look = w.districts.at(lot.center).and_then(|d| d.look_for(b.land_use));
        use nd_sim::districts::Look;
        use nd_sim::LandUse;
        neon += match (b.land_use, look) {
            (_, Some(Look::NeonStrip | Look::NightMarket | Look::Kowloon | Look::Lantern)) => 1.0,
            (LandUse::Entertainment, _) => 0.8,
            (LandUse::Commercial, _) => 0.3,
            _ => 0.0,
        };
        green += match (b.land_use, look) {
            (LandUse::Park, _) => 1.0,
            (_, Some(Look::Suburban | Look::Garden | Look::Eco | Look::SolarpunkGreen)) => 0.6,
            (LandUse::Residential, _) if b.floors <= 2 => 0.35,
            _ => 0.0,
        };
        if b.land_use == LandUse::Industrial {
            industry += 1.0;
        }
    }
    let area_ha = (std::f32::consts::PI * b2 / 10_000.0).max(0.1);
    let density = 1.0 - (-(floors / area_ha) / 60.0).exp();
    let share = |x: f32| if n > 0.0 { x / n } else { 0.0 };
    // Water: a 5 x 5 grid over the radius.
    let mut wet = 0;
    for i in -2..=2 {
        for j in -2..=2 {
            let p = focus + SVec2::new(i as f32, j as f32) * (br * 0.45);
            if w.landscape.is_water(p) {
                wet += 1;
            }
        }
    }
    let water = wet as f32 / 25.0;
    // Open country with nothing built counts as green.
    let open = (1.0 - density) * (1.0 - water) * if n < 3.0 { 0.8 } else { 0.4 };
    let precip = crate::render::climate::precip_shown(w, lighting);
    Surroundings {
        height,
        cars_close,
        cars_near,
        car_speed: if cars_near > 0.0 { speed_sum / cars_near } else { 0.0 },
        radius,
        walkers_close,
        walkers_near,
        density,
        neon: share(neon),
        green: share(green).max(open),
        industry: share(industry),
        water,
        daylight: daylight(w, lighting),
        rain: precip.filter(|p| p.0 == "rain").map_or(0.0, |p| p.1),
        snow: precip.filter(|p| p.0 == "snow").map_or(0.0, |p| p.1),
        wind: w.weather.wind,
        underground,
    }
}

/// Daylight as the renderer lights it: 1 by day, 0 at night (the clock,
/// or the View window's hour when it does not follow the clock).
fn daylight(w: &nd_sim::World, lighting: &Lighting) -> f32 {
    let t = if lighting.follow_clock { w.clock.day_fraction() } else { (lighting.hour / 24.0).rem_euclid(1.0) };
    let elev = ((t - 0.25) * std::f32::consts::TAU).sin();
    nd_audio::dsp::smoothstep(-0.12, 0.30, elev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_radius_grows_with_height_within_bounds() {
        assert_eq!(radius_for(1.7), CLOSE);
        assert!(radius_for(300.0) > radius_for(100.0));
        assert_eq!(radius_for(1e6), 1_500.0);
    }
}
