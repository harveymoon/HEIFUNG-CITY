//! [look] The street at eye level: lamps that light the road at night, roads
//! that go dark and glossy in the rain, and how far you see (and in what
//! colour) by climate, weather and hour.
//!
//! - **Lamps.** Every lamp post records its lantern (`Geometry::lamps`); at
//!   night a small pool of real point lights (no shadows, clustered) is hung
//!   on the lamps nearest the view, fading out at the pool's edge so moving
//!   the camera does not pop them. The glowing lanterns far away stay what
//!   they were: glow. None at Low.
//! - **Wet roads.** While it rains the asphalt and paving darken and lose
//!   most of their roughness (the roughness maps keep their variation, so
//!   worn patches stay dull and the smooth ones read as puddles). They dry
//!   slowly after the rain. Snow climates stay dry.
//! - **Fog.** The haze takes a climate's tint (dusty in the desert, cold
//!   and pale in the arctic, wet green-grey in the tropics, sulphur grey on
//!   volcanic maps) and closes in under rain, snow and at night in humid
//!   climates; the falloff follows the camera (walk view sees less).

use bevy::light::NotShadowCaster;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use nd_procgen::Surface;
use nd_sim::Climate;

use super::look::Quality;
use super::{sky, smoothstep, Lighting, Materials, RenderStats};
use crate::camera::{CameraMode, OrbitCamera};
use crate::sim::Sim;

/// The lamp heads of the street network (surface layer), as last built.
#[derive(Resource, Default)]
pub(super) struct StreetLamps {
    pub lamps: Vec<Vec3>,
}

#[derive(Component)]
pub(super) struct LampLight;

/// Most real lights at once, by tier.
pub(super) fn lamp_pool(tier: Quality) -> usize {
    match tier {
        Quality::High => 40,
        Quality::Medium => 18,
        Quality::Low => 0,
    }
}
/// Farthest a lamp gets a real light, metres from the focus.
const LAMP_REACH: f32 = 320.0;
/// Light of one lamp (lumens; the scene's sun is 11 klx by day, the moon
/// 650 lx, so a lamp has to be bright to show under it).
const LAMP_LUMENS: f32 = 2.6e6;
const LAMP_RANGE: f32 = 30.0;
/// Sodium-white.
const LAMP_RGB: [f32; 3] = [1.0, 0.72, 0.45];

/// The point the lamps gather round: the walker's eye, or the orbit
/// camera's target (what the player is looking at), pulled towards the eye
/// when it is low.
pub(super) fn lamp_focus(mode: CameraMode, eye: Vec3, cam: &OrbitCamera) -> Vec3 {
    match mode {
        CameraMode::Walk => eye,
        CameraMode::Orbit => {
            let low = 1.0 - smoothstep(30.0, 160.0, eye.y - cam.target.y);
            cam.target.lerp(eye, low * 0.5)
        }
    }
}

/// How much night it is for the lamps, 0..1.
pub(super) fn lamp_night(daylight: f32) -> f32 {
    1.0 - smoothstep(0.2, 0.6, daylight)
}

/// The `n` lamps nearest `focus` within reach, nearest first, with their
/// distance.
pub(super) fn nearest_lamps(lamps: &[Vec3], focus: Vec3, n: usize, out: &mut Vec<(f32, Vec3)>) {
    out.clear();
    if n == 0 {
        return;
    }
    out.extend(lamps.iter().map(|p| (p.distance(focus), *p)).filter(|(d, _)| *d < LAMP_REACH));
    if out.len() > n {
        out.select_nth_unstable_by(n - 1, |a, b| a.0.total_cmp(&b.0));
        out.truncate(n);
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_lamp_lights(
    mut commands: Commands,
    lamps: Res<StreetLamps>,
    look: Res<sky::SkyLook>,
    stats: Res<RenderStats>,
    lighting: Res<Lighting>,
    mode: Res<CameraMode>,
    cams: Query<(&GlobalTransform, &OrbitCamera)>,
    mut pool: Query<(&mut PointLight, &mut Transform, &mut Visibility), With<LampLight>>,
    mut near: Local<Vec<(f32, Vec3)>>,
    mut spawned: Local<usize>,
) {
    let want = lamp_pool(stats.graphics.tier);
    if *spawned < want {
        for _ in *spawned..want {
            commands.spawn((
                PointLight { color: Color::linear_rgb(LAMP_RGB[0], LAMP_RGB[1], LAMP_RGB[2]), intensity: 0.0, range: LAMP_RANGE, radius: 0.25, shadow_maps_enabled: false, ..default() },
                super::offstage(),
                Visibility::Hidden,
                NotShadowCaster,
                LampLight,
            ));
        }
        *spawned = want;
        return;
    }
    let night = lamp_night(look.daylight);
    let on = night > 0.01 && !lighting.heat;
    let Some((tf, cam)) = cams.iter().next() else { return };
    if on {
        nearest_lamps(&lamps.lamps, lamp_focus(*mode, tf.translation(), cam), want, &mut near);
    } else {
        near.clear();
    }
    // Fade the farthest of the pool out, so a lamp entering or leaving it
    // eases in rather than pops.
    let edge = near.last().map(|n| n.0).unwrap_or(0.0).max(30.0);
    let mut lit = near.iter();
    for (mut light, mut ltf, mut vis) in &mut pool {
        match lit.next() {
            Some((d, p)) => {
                let fade = 1.0 - smoothstep(edge * 0.75, edge * 1.001, *d) * if near.len() >= want { 1.0 } else { 0.0 };
                let k = LAMP_LUMENS * night * fade.max(0.0);
                if (light.intensity - k).abs() > k * 0.01 + 1.0 {
                    light.intensity = k;
                }
                if ltf.translation != *p {
                    ltf.translation = *p;
                }
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
            None => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

// ── wet roads ───────────────────────────────────────────────────────────────

/// The street surfaces that get wet.
const WET_SURFACES: [Surface; 5] = [Surface::Asphalt, Surface::WornAsphalt, Surface::Paving, Surface::WornPaving, Surface::Tactile];
/// Seconds (real) to soak through at full rain, and to dry.
const WET_UP_S: f32 = 12.0;
const DRY_S: f32 = 45.0;

/// How a surface looks at wetness `wet` (0..1): its base colour multiplier
/// and its roughness multiplier (the maps hold the absolute roughness).
pub(super) fn wet_look(wet: f32) -> (f32, f32) {
    let wet = wet.clamp(0.0, 1.0);
    (1.0 - 0.45 * wet, 1.0 - 0.78 * wet)
}

pub(super) fn sync_wet_roads(sim: Res<Sim>, time: Res<Time>, mats: Res<Materials>, mut materials: ResMut<Assets<StandardMaterial>>, mut wet: Local<f32>, mut applied: Local<Option<i32>>) {
    let rain = super::climate::precip_now(&sim.world).filter(|(k, _)| *k == "rain").map(|(_, i)| i).unwrap_or(0.0);
    let dt = time.delta_secs().min(0.25);
    *wet = if rain > *wet { (*wet + dt / WET_UP_S).min(rain.max(0.35)) } else { (*wet - dt / DRY_S).max(rain) };
    let key = (*wet * 48.0).round() as i32;
    if *applied == Some(key) {
        return;
    }
    *applied = Some(key);
    let (dark, rough) = wet_look(key as f32 / 48.0);
    for s in WET_SURFACES {
        if let Some(mut m) = materials.get_mut(&mats.surface(s)) {
            m.base_color = Color::linear_rgb(dark, dark, dark);
            m.perceptual_roughness = rough;
        }
    }
}

// ── fog by climate, weather and hour ────────────────────────────────────────

/// A climate's haze: its tint (relative; luminance is kept) and how far
/// one sees in clear weather against the temperate default.
pub(super) fn climate_haze(c: Climate) -> ([f32; 3], f32) {
    match c {
        Climate::Temperate => ([1.0, 1.0, 1.0], 1.0),
        Climate::Desert => ([1.18, 1.0, 0.78], 0.9),
        Climate::Tropical => ([0.92, 1.04, 0.98], 0.78),
        Climate::Arctic => ([0.9, 0.98, 1.14], 0.86),
        Climate::Volcanic => ([1.04, 1.0, 0.9], 0.72),
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The fog colour `update_daylight` chose, tinted for the climate (most by
/// day; the city's glow owns the night) and greyed under rain or snow.
pub(super) fn tint_fog(fog: [f32; 3], climate: Climate, daylight: f32, precip: f32) -> [f32; 3] {
    let (tint, _) = climate_haze(climate);
    let l = lum(fog);
    let tl = lum(tint).max(1e-4);
    let k = 0.25 + 0.75 * daylight;
    let mut out = [0.0; 3];
    for i in 0..3 {
        let tinted = fog[i] * (1.0 + (tint[i] / tl - 1.0) * k);
        // Rain and snow: towards a flat grey of the same brightness, a
        // little darker.
        out[i] = (tinted + (l - tinted) * 0.6 * precip) * (1.0 - 0.15 * precip * daylight);
    }
    out
}

/// How far the fog lets you see, against the clear default: climate,
/// rain or snow, and a humid night's mist.
pub(super) fn visibility(climate: Climate, daylight: f32, precip: f32) -> f32 {
    let (_, clear) = climate_haze(climate);
    let humid = matches!(climate, Climate::Tropical | Climate::Temperate) as u8 as f32;
    clear * (1.0 - 0.5 * precip) * (1.0 - 0.15 * humid * (1.0 - daylight))
}

/// The falloff for the camera mode, scaled by [`visibility`]. Walk view
/// sees less (the far ground is not drawn at street level).
pub(super) fn fog_falloff(mode: CameraMode, vis: f32) -> (f32, f32) {
    let sight = nd_sim::landscape::SIGHT;
    match mode {
        CameraMode::Walk => (120.0 * vis, 900.0 * vis),
        // The far end stays at the edge of what is drawn, a little nearer
        // in thick weather.
        CameraMode::Orbit => (500.0 * vis, sight * (0.75 + 0.25 * vis).min(1.0)),
    }
}

pub(super) fn sync_fog_reach(sim: Res<Sim>, look: Res<sky::SkyLook>, mode: Res<CameraMode>, lighting: Res<Lighting>, mut fogs: Query<&mut DistanceFog, With<OrbitCamera>>) {
    if lighting.heat {
        return;
    }
    let w = &sim.world;
    let precip = super::climate::precip_now(w).map(|(_, i)| i).unwrap_or(0.0);
    let vis = visibility(w.landscape.params.climate, look.daylight, precip);
    let (start, end) = fog_falloff(*mode, vis);
    for mut fog in &mut fogs {
        let differs = match fog.falloff {
            FogFalloff::Linear { start: s, end: e } => (s - start).abs() > 1.0 || (e - end).abs() > 1.0,
            _ => true,
        };
        if differs {
            fog.falloff = FogFalloff::Linear { start, end };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_lamps_picks_the_closest_in_reach() {
        let lamps: Vec<Vec3> = (0..50).map(|i| Vec3::new(i as f32 * 10.0, 7.0, 0.0)).collect();
        let mut out = Vec::new();
        nearest_lamps(&lamps, Vec3::new(105.0, 1.7, 0.0), 4, &mut out);
        assert_eq!(out.len(), 4);
        let xs: Vec<f32> = out.iter().map(|(_, p)| p.x).collect();
        assert!(xs[0] == 100.0 || xs[0] == 110.0, "{xs:?}");
        assert!(out.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!(xs.iter().all(|x| (80.0..=130.0).contains(x)));
        // Out of reach: none.
        nearest_lamps(&lamps, Vec3::new(5000.0, 0.0, 0.0), 4, &mut out);
        assert!(out.is_empty());
        // Low tier: no pool.
        nearest_lamps(&lamps, Vec3::ZERO, lamp_pool(Quality::Low), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn lamps_are_off_by_day_and_on_at_night() {
        assert_eq!(lamp_night(1.0), 0.0);
        assert_eq!(lamp_night(0.0), 1.0);
    }

    #[test]
    fn wet_roads_are_darker_and_glossier() {
        assert_eq!(wet_look(0.0), (1.0, 1.0));
        let (dark, rough) = wet_look(1.0);
        assert!(dark < 0.7 && rough < 0.3);
    }

    #[test]
    fn fog_closes_in_under_rain_and_keeps_its_brightness() {
        for c in Climate::ALL {
            assert!(visibility(c, 1.0, 1.0) < visibility(c, 1.0, 0.0));
            let fog = [0.5, 0.55, 0.6];
            let t = tint_fog(fog, c, 1.0, 0.0);
            assert!((lum(t) - lum(fog)).abs() < 0.06, "{c:?}: {t:?}");
        }
        assert_eq!(tint_fog([0.3, 0.3, 0.3], Climate::Temperate, 1.0, 0.0), [0.3, 0.3, 0.3]);
        let (s, e) = fog_falloff(CameraMode::Walk, 1.0);
        assert!(s < e && e < fog_falloff(CameraMode::Orbit, 1.0).1);
    }
}
