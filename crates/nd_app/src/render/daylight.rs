//! The sun, sky light, fog and glow through the day, and the sun's shadow cascades.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

/// Sun and sky control. By default the lighting follows the sim clock; the
/// player can pin the time of day instead.
#[derive(Resource)]
pub struct Lighting {
    pub follow_clock: bool,
    /// Hour of day used when not following the clock, 0..24.
    pub hour: f32,
    /// Drone heat vision: the city through a thermal camera (see
    /// `render/heat.rs`). A view mode like the time of day, so it lives here.
    pub heat: bool,
    /// [look] The graphics setting: Auto (the throttler holds the frame
    /// rate) or a fixed tier. `ND_QUALITY` sets the starting value.
    pub quality: QualitySetting,
    /// [show-weather] Draw the weather: falling rain and snow, the fog it
    /// brings and wet streets (View window, ALSO DRAWN). Off hides them
    /// only; the sim's weather goes on.
    pub show_weather: bool,
}

impl Default for Lighting {
    fn default() -> Self {
        Self { follow_clock: true, hour: 14.0, heat: false, quality: QualitySetting::from_env(), show_weather: true }
    }
}
/// Near shadow cascade bounds (m): the defaults, kept while the orbit
/// camera is close enough for them to cover what it sees.
pub(super) const SHADOW_FAR: f32 = 650.0;
pub(super) const SHADOW_FIRST: f32 = 45.0;

/// [sweep-visual] The sun's cascades for the camera: the defaults up close;
/// zoomed out past where the 650 m cascade reaches, they stretch with the
/// orbit distance (to 3 km), so a long view over the skyline keeps its
/// towers' shadows instead of losing every shadow at once. Walk view keeps
/// the defaults. Only rewritten when the reach moves by a tenth.
pub(super) fn sun_cascades_follow_camera(
    mode: Res<crate::camera::CameraMode>,
    cams: Query<&crate::camera::OrbitCamera>,
    mut sun: Query<&mut bevy::light::CascadeShadowConfig, With<SunTag>>,
    mut last: Local<f32>,
) {
    let d = if mode.street() { 0.0 } else { cams.iter().next().map_or(0.0, |c| c.distance) };
    let far = (d * 1.9).clamp(SHADOW_FAR, 3000.0);
    if *last > 0.0 && (far / *last - 1.0).abs() < 0.1 {
        return;
    }
    *last = far;
    let first = SHADOW_FIRST * far / SHADOW_FAR;
    for mut c in sun.iter_mut() {
        *c = CascadeShadowConfigBuilder { num_cascades: 3, minimum_distance: 0.5, maximum_distance: far, first_cascade_far_bound: first, ..default() }.build();
    }
}

/// How dark it is, `0` (day) to `1` (night), from the same sun the
/// lighting uses.
pub(super) fn night_strength(sim: &Sim, lighting: &Lighting) -> f32 {
    let t = if lighting.follow_clock { sim.world.clock.day_fraction() } else { (lighting.hour / 24.0).rem_euclid(1.0) };
    let elev = ((t - 0.25) * std::f32::consts::TAU).sin();
    1.0 - smoothstep(-0.12, 0.30, elev)
}

/// Sun, ambient light, sky, fog and glow strength follow the sim clock.
///
/// Everything is written only when the quantised time of day moves, which
/// is far below a visible step, and the shared Glow material (whose rewrite re-uploads it and breaks the
/// batches of every glow mesh) is rounded to 1/64 of its range, so a full
/// day costs at most a few dozen material updates instead of one per frame.
pub(super) fn update_daylight(
    sim: Res<Sim>,
    lighting: Res<Lighting>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut window_materials: ResMut<Assets<lights::WindowMaterial>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<SunTag>>,
    mut fog: Query<&mut DistanceFog>,
    mut sky_look: ResMut<sky::SkyLook>,
    mut blooms: Query<&mut bevy::post_process::bloom::Bloom>,
) {
    let pal = &assets.palette;
    let t = if lighting.follow_clock { sim.world.clock.day_fraction() } else { (lighting.hour / 24.0).rem_euclid(1.0) };
    // Quantised to 1/4096 of a day (about 20 sim seconds, a 0.09 degree
    // step of the sun), so at 1x speed the lighting is written a few times a
    // second rather than every frame.
    // [look] The fog's tint follows the climate and the rain or snow too.
    let climate = sim.world.landscape.params.climate;
    let precip = climate::precip_shown(&sim.world, &lighting).map(|(_, i)| i).unwrap_or(0.0);
    let climate_ix = nd_sim::Climate::ALL.iter().position(|c| *c == climate).unwrap_or(0) as i32;
    let daylight_key = (t * 4096.0).round() as i32 + 8192 * (climate_ix * 16 + (precip * 15.0).round() as i32);
    if state.daylight_key == Some(daylight_key) && !assets.palette_changed {
        return;
    }
    state.daylight_key = Some(daylight_key);
    let elev = ((t - 0.25) * std::f32::consts::TAU).sin();
    let daylight = smoothstep(-0.12, 0.30, elev);
    let low_sun = 1.0 - smoothstep(0.05, 0.45, elev.abs());
    let dusk = (1.0 - smoothstep(0.0, 0.22, elev.abs())) * 0.8;

    let sun_color = lerp3(lerp3(pal.sun_low, pal.sun, 1.0 - low_sun), pal.moon, 1.0 - daylight);
    // Moonlight plus the city's own glow off the haze: night stays night
    // (the sky is dark, the shadows soft and blue) but streets read.
    let illuminance = 650.0 + 10350.0 * daylight;
    let night = 1.0 - daylight;
    // Light pollution grows with the city: a village barely tints its sky,
    // a few hundred lit buildings make the low clouds glow.
    let city = (sim.world.buildings.len() as f32 / 350.0).clamp(0.2, 1.0);
    let pollution = night * city;
    // Sodium orange under neon magenta, as the haze over a big city at night.
    let glow_color = [0.26, 0.10, 0.22];
    let azimuth = (t - 0.25) * std::f32::consts::TAU;
    let sun_dir = if elev > 0.02 {
        let r = (1.0 - elev * elev).sqrt();
        Vec3::new(azimuth.cos() * r, elev, azimuth.sin() * r)
    } else {
        Vec3::new(0.4, 0.75, -0.5)
    };
    for (mut light, mut tf) in &mut sun {
        light.color = Color::linear_rgb(sun_color[0], sun_color[1], sun_color[2]);
        light.illuminance = illuminance;
        tf.translation = sun_dir * 1000.0;
        tf.look_at(Vec3::ZERO, Vec3::Y);
    }

    let sky = lerp3(lerp3(pal.sky, pal.sky_day, daylight), pal.sky_dusk, dusk * 0.6);
    let fog_c = lerp3(lerp3(pal.fog, pal.fog_day, daylight), pal.sky_dusk, dusk * 0.5);
    // At night the haze is lit from below by the city.
    let fog_c = [fog_c[0] + glow_color[0] * 0.15 * pollution, fog_c[1] + glow_color[1] * 0.15 * pollution, fog_c[2] + glow_color[2] * 0.15 * pollution];
    let fog_c = street_look::tint_fog(fog_c, climate, daylight, precip);
    // The zenith stays dark and a little blue at night.
    let zenith = lerp3(sky, [0.003, 0.004, 0.016], night * 0.85);
    clear.0 = Color::linear_rgb(zenith[0], zenith[1], zenith[2]);
    // Scattering: the fog brightens towards the sun (low sun most), so
    // distant haze glows at dusk; the moon lends a faint cold halo.
    let glare = lerp3(lerp3(pal.sun_low, pal.sun, 1.0 - low_sun), pal.moon, night);
    let glare_k = 0.08 + 0.55 * low_sun * daylight.max(0.25) - 0.05 * night;
    for mut f in &mut fog {
        f.color = Color::linear_rgb(fog_c[0], fog_c[1], fog_c[2]);
        f.directional_light_color = Color::linear_rgba(glare[0] * glare_k, glare[1] * glare_k, glare[2] * glare_k, 0.6);
        f.directional_light_exponent = 18.0;
    }
    // The dome: fog at the horizon, the pollution band, the dark zenith,
    // glare round the sun or moon.
    let band = lerp3(lerp3(fog_c, [fog_c[0] * 1.15, fog_c[1] * 1.15, fog_c[2] * 1.2], daylight), [fog_c[0] + glow_color[0] * 0.35 * pollution, fog_c[1] + glow_color[1] * 0.35 * pollution, fog_c[2] + glow_color[2] * 0.35 * pollution], night);
    let next = sky::SkyLook {
        zenith,
        horizon: fog_c,
        glow: band,
        glow_height: 0.12 + 0.18 * pollution + 0.25 * daylight,
        // The real sun while it is near or over the horizon (its glow
        // lingers after sunset), else the moon the night light comes from.
        sun_dir: if elev > -0.2 {
            let r = (1.0 - elev * elev).max(0.0).sqrt();
            Vec3::new(azimuth.cos() * r, elev.max(0.0), azimuth.sin() * r).normalize_or_zero()
        } else {
            sun_dir.normalize_or_zero()
        },
        sun_glare: { let k = 0.35 * daylight + 0.9 * dusk + 0.04 * night; [glare[0] * k, glare[1] * k, glare[2] * k] },
        daylight,
        version: sky_look.version + 1,
    };
    *sky_look = next;
    // The city's light bouncing back off the haze fills the shadows at
    // night with a warm violet; by day the sky's own blue-white.
    // [look] The sky's environment light (`look`) now brings the sky's own
    // colour to the shadows as well; the flat ambient is what remains of
    // the fill (light bounced off the city), so shadows keep their depth.
    ambient.brightness = 0.6 * (95.0 + 105.0 * pollution) + 420.0 * daylight;
    let amb_night = lerp3([0.35, 0.45, 0.9], [0.85, 0.5, 0.85], 0.45 * city);
    let amb = lerp3(amb_night, [0.8, 0.85, 1.0], daylight);
    ambient.color = Color::linear_rgb(amb[0], amb[1], amb[2]);
    // Bloom: neon and windows bleed into the haze at night, barely by day.
    // Heat vision keeps its own (it stores and restores the camera's).
    if !lighting.heat {
        for mut b in &mut blooms {
            b.intensity = 0.07 + 0.09 * night;
            b.low_frequency_boost = 0.45 + 0.2 * night;
        }
    }

    let glow_key = ((1.0 - 0.7 * daylight) * 64.0).round() as i32;
    if state.glow_key != Some(glow_key) || assets.palette_changed {
        state.glow_key = Some(glow_key);
        let glow_k = glow_key as f32 / 64.0;
        if let Some(mut m) = materials.get_mut(&mats.glow()) {
            m.base_color = Color::linear_rgb(glow_k, glow_k, glow_k);
        }
        if let Some(mut m) = window_materials.get_mut(&mats.windows) {
            m.base.base_color = Color::linear_rgb(glow_k, glow_k, glow_k);
        }
        // Road paint goes the other way from glow: full at night (as the
        // markings always were) and brighter by day, so lane lines and
        // arrows read as white paint on sunlit asphalt, not dark grey.
        let paint_k = 1.0 + 0.8 * daylight;
        if let Some(mut m) = materials.get_mut(&mats.surface(Surface::Paint)) {
            m.base_color = Color::linear_rgb(paint_k, paint_k, paint_k);
        }
    }
}
