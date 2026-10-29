//! What is around the listener, and the bed settings it calls for: pure
//! functions, sampled by the game a few times a second and smoothed by the
//! engine.

use crate::dsp::smoothstep;

/// What the game reads from around the camera: plain numbers so this crate
/// knows nothing of the sim (and cannot touch it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surroundings {
    /// The camera's height over the ground, metres.
    pub height: f32,
    /// Moving cars within ~60 m of the spot under the camera.
    pub cars_close: f32,
    /// Moving cars within [`Surroundings::radius`].
    pub cars_near: f32,
    /// Their mean speed, m/s.
    pub car_speed: f32,
    /// The area counted for `cars_near` and `walkers_near`, metres.
    pub radius: f32,
    /// People on foot within ~60 m.
    pub walkers_close: f32,
    /// People on foot within the radius.
    pub walkers_near: f32,
    /// How built up it is, `0..1` (floors and lots around).
    pub density: f32,
    /// Neon and nightlife: neon strips, night markets, entertainment, `0..1`.
    pub neon: f32,
    /// Parks, gardens, suburbs, open country, `0..1`.
    pub green: f32,
    /// Works and yards, `0..1`.
    pub industry: f32,
    /// Share of the ground around that is water, `0..1`.
    pub water: f32,
    /// 1 at noon, 0 in the night.
    pub daylight: f32,
    /// Rain falling, `0..1`.
    pub rain: f32,
    /// Snow falling, `0..1`.
    pub snow: f32,
    /// The weather's wind, `0..1` (0.45 an ordinary day, 1 a gale).
    pub wind: f32,
    /// The Underground view (tunnels): everything muffled.
    pub underground: bool,
}

impl Default for Surroundings {
    fn default() -> Self {
        Self {
            height: 150.0,
            cars_close: 0.0,
            cars_near: 0.0,
            car_speed: 0.0,
            radius: 200.0,
            walkers_close: 0.0,
            walkers_near: 0.0,
            density: 0.0,
            neon: 0.0,
            green: 0.0,
            industry: 0.0,
            water: 0.0,
            daylight: 1.0,
            rain: 0.0,
            snow: 0.0,
            wind: 0.45,
            underground: false,
        }
    }
}

/// The beds' settings, each `0..1` unless said otherwise. The engine eases
/// towards a new set over a second or two, so they may jump.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BedParams {
    /// The three beds' crossfade weights (equal power): high, mid, street.
    pub high: f32,
    pub mid: f32,
    pub street: f32,
    /// High bed: the distant city's wash and its brightness.
    pub wash: f32,
    pub wash_tone: f32,
    /// Wind (any height, most up high) and how gusty.
    pub wind: f32,
    pub gust: f32,
    /// Mid bed: the traffic roar and its brightness (speed).
    pub traffic: f32,
    pub traffic_tone: f32,
    /// Street bed: tyre hiss, pass-bys a second, crowd murmur, footsteps a
    /// second, air-conditioning hum, neon buzz.
    pub tyres: f32,
    pub passby_rate: f32,
    pub murmur: f32,
    pub steps_rate: f32,
    pub hum: f32,
    pub neon: f32,
    /// Nature: birds by day, crickets at night.
    pub birds: f32,
    pub crickets: f32,
    /// Weather and water.
    pub rain: f32,
    pub water: f32,
    /// How muffled everything is (snow, the Underground view): 0 open.
    pub muffle: f32,
    /// The ambience as a whole.
    pub level: f32,
}

/// Where the beds hand over, metres over the ground: street up to ~30 m,
/// mid around 60–200 m, high from ~600 m.
pub const STREET_TOP: f32 = 30.0;
pub const MID_LOW: f32 = 70.0;
pub const MID_HIGH: f32 = 220.0;
pub const HIGH_FULL: f32 = 650.0;

/// The three beds' weights at a camera height, crossfaded on a log scale
/// with equal power (the squares sum to 1, so the sum keeps its loudness
/// through the crossfade).
pub fn height_weights(height: f32) -> [f32; 3] {
    let h = height.max(0.5).ln();
    let up = smoothstep(STREET_TOP.ln(), MID_LOW.ln(), h);
    let top = smoothstep(MID_HIGH.ln(), HIGH_FULL.ln(), h);
    // Shares: street (1-up), mid (up - top), high (top); then to power.
    let s = 1.0 - up;
    let m = up * (1.0 - top);
    let t = up * top;
    [t.sqrt(), m.sqrt(), s.sqrt()]
}

/// Saturating count: 0 at none, ~0.63 at `k`, towards 1 at many.
fn sat(x: f32, k: f32) -> f32 {
    1.0 - (-x.max(0.0) / k).exp()
}

/// The beds a set of surroundings calls for.
pub fn bed_params(s: &Surroundings) -> BedParams {
    let [high, mid, street] = height_weights(s.height);
    let day = s.daylight.clamp(0.0, 1.0);
    let night = 1.0 - day;
    let density = s.density.clamp(0.0, 1.0);
    let neon = s.neon.clamp(0.0, 1.0);
    let green = s.green.clamp(0.0, 1.0);
    let rain = s.rain.clamp(0.0, 1.0);
    let snow = s.snow.clamp(0.0, 1.0);
    let wind_w = s.wind.clamp(0.0, 1.0);
    // Cars a hectare in the counted area.
    let area_ha = (std::f32::consts::PI * s.radius.max(30.0).powi(2) / 10_000.0).max(0.3);
    let car_density = s.cars_near / area_ha;
    let traffic = sat(car_density, 1.2);
    // Faster traffic roars brighter: 0 at a crawl, 1 at ~25 m/s.
    let speed = (s.car_speed / 25.0).clamp(0.0, 1.0);
    let walk_density = s.walkers_near / area_ha;
    // The city as a whole from up high: built-up land and its traffic.
    let wash = (0.25 + 0.45 * density + 0.3 * traffic).min(1.0) * (1.0 - 0.5 * green * (1.0 - density));
    // Wind: the weather's, stronger aloft, a little in the streets.
    let wind = sat(wind_w, 0.6) * (0.35 + 0.65 * high + 0.25 * mid) * (1.0 - 0.3 * street * density);
    let tyres = sat(s.cars_close, 5.0) * (0.55 + 0.45 * speed) + 0.3 * rain * sat(s.cars_close, 3.0);
    let murmur = sat(s.walkers_close, 14.0).max(0.35 * sat(walk_density, 6.0)) * (0.8 + 0.4 * neon).min(1.0);
    // Birds sing in daylight over green; crickets at night; neither in a
    // dense neon strip or the rain.
    let nature = green * (1.0 - 0.7 * density) * (1.0 - 0.8 * rain) * (1.0 - snow);
    let birds = nature * smoothstep(0.35, 0.75, day);
    let crickets = nature * smoothstep(0.45, 0.85, night) * (1.0 - 0.5 * wind_w);
    let muffle = if s.underground { 0.85 } else { 0.55 * snow };
    BedParams {
        high,
        mid,
        street,
        wash,
        wash_tone: (0.3 + 0.4 * density + 0.3 * neon * night).min(1.0) * (1.0 - 0.4 * snow),
        wind: wind * (1.0 - 0.3 * snow),
        gust: (0.2 + 0.8 * wind_w).min(1.0),
        traffic: traffic * (0.75 + 0.25 * speed),
        traffic_tone: speed,
        tyres: tyres.min(1.0),
        passby_rate: s.cars_close.max(0.0).min(40.0) * (0.05 + 0.1 * speed),
        murmur,
        steps_rate: s.walkers_close.max(0.0).min(60.0) * 0.15,
        hum: (0.15 + 0.85 * density) * (1.0 - 0.7 * green) * (0.6 + 0.4 * day),
        neon: neon * (0.25 + 0.75 * night),
        birds,
        crickets,
        rain,
        water: sat(s.water, 0.15),
        muffle,
        level: if s.underground { 0.55 } else { 1.0 - 0.25 * snow },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_weights_hand_over_street_mid_high_with_equal_power() {
        let w = height_weights(2.0);
        assert!(w[2] > 0.999 && w[0] < 1e-3, "{w:?}");
        let w = height_weights(120.0);
        assert!(w[1] > 0.99, "{w:?}");
        let w = height_weights(2_000.0);
        assert!(w[0] > 0.999, "{w:?}");
        for h in [1.0, 10.0, 40.0, 55.0, 90.0, 300.0, 500.0, 900.0, 5_000.0] {
            let w = height_weights(h);
            let power: f32 = w.iter().map(|x| x * x).sum();
            assert!((power - 1.0).abs() < 1e-4, "{h}: {w:?}");
        }
        // Rising, the street fades and the high bed comes in.
        let mut last = height_weights(1.0);
        for h in (1..200).map(|i| i as f32 * 15.0) {
            let w = height_weights(h);
            assert!(w[2] <= last[2] + 1e-6 && w[0] >= last[0] - 1e-6);
            last = w;
        }
    }

    #[test]
    fn busy_traffic_is_louder_and_faster_traffic_brighter() {
        let quiet = bed_params(&Surroundings { height: 120.0, cars_near: 2.0, car_speed: 5.0, ..Default::default() });
        let busy = bed_params(&Surroundings { height: 120.0, cars_near: 80.0, car_speed: 5.0, ..Default::default() });
        let fast = bed_params(&Surroundings { height: 120.0, cars_near: 80.0, car_speed: 22.0, ..Default::default() });
        assert!(busy.traffic > quiet.traffic * 3.0, "{} vs {}", busy.traffic, quiet.traffic);
        assert!(fast.traffic_tone > busy.traffic_tone);
        assert!(fast.traffic >= busy.traffic);
        let close = bed_params(&Surroundings { height: 3.0, cars_close: 12.0, car_speed: 12.0, ..Default::default() });
        assert!(close.tyres > 0.5 && close.passby_rate > 0.5, "{close:?}");
        assert_eq!(bed_params(&Surroundings::default()).tyres, 0.0);
    }

    #[test]
    fn neon_buzzes_at_night_and_parks_have_birds_by_day_and_crickets_at_night() {
        let strip = Surroundings { height: 3.0, neon: 1.0, density: 0.9, ..Default::default() };
        let strip_night = bed_params(&Surroundings { daylight: 0.0, ..strip });
        let strip_day = bed_params(&Surroundings { daylight: 1.0, ..strip });
        assert!(strip_night.neon > 0.9 && strip_day.neon < 0.3);
        assert_eq!(strip_night.birds, 0.0);
        let park = Surroundings { height: 3.0, green: 1.0, ..Default::default() };
        let day = bed_params(&Surroundings { daylight: 1.0, ..park });
        let night = bed_params(&Surroundings { daylight: 0.0, ..park });
        assert!(day.birds > 0.9 && day.crickets == 0.0, "{day:?}");
        assert!(night.crickets > 0.4 && night.birds == 0.0, "{night:?}");
        // Suburbs and parks hum less than downtown.
        assert!(day.hum < strip_day.hum);
        // Rain silences the birds and brings the rain bed.
        let wet = bed_params(&Surroundings { daylight: 1.0, rain: 1.0, ..park });
        assert!(wet.birds < day.birds * 0.3 && wet.rain == 1.0);
    }

    #[test]
    fn snow_and_the_underground_muffle_and_wind_follows_the_weather() {
        let open = bed_params(&Surroundings::default());
        let snow = bed_params(&Surroundings { snow: 1.0, ..Default::default() });
        let under = bed_params(&Surroundings { underground: true, ..Default::default() });
        assert_eq!(open.muffle, 0.0);
        assert!(snow.muffle > 0.4 && under.muffle > snow.muffle);
        assert!(under.level < open.level);
        let calm = bed_params(&Surroundings { wind: 0.1, height: 900.0, ..Default::default() });
        let gale = bed_params(&Surroundings { wind: 1.0, height: 900.0, ..Default::default() });
        assert!(gale.wind > calm.wind * 2.0 && gale.gust > calm.gust);
        let shore = bed_params(&Surroundings { water: 0.5, ..Default::default() });
        assert!(shore.water > 0.9 && open.water == 0.0);
    }

    #[test]
    fn every_parameter_stays_in_range_for_wild_inputs() {
        for s in [
            Surroundings { height: -5.0, cars_close: 1e6, cars_near: 1e6, car_speed: 1e3, radius: 0.0, walkers_close: 1e6, walkers_near: 1e6, density: 9.0, neon: 9.0, green: 9.0, industry: 9.0, water: 9.0, daylight: 9.0, rain: 9.0, snow: 9.0, wind: 9.0, underground: true },
            Surroundings { height: 1e7, cars_close: -1.0, cars_near: -1.0, car_speed: -1.0, radius: -1.0, walkers_close: -1.0, walkers_near: -1.0, density: -1.0, neon: -1.0, green: -1.0, industry: -1.0, water: -1.0, daylight: -1.0, rain: -1.0, snow: -1.0, wind: -1.0, underground: false },
        ] {
            let p = bed_params(&s);
            for (name, v) in [
                ("high", p.high), ("mid", p.mid), ("street", p.street), ("wash", p.wash), ("wash_tone", p.wash_tone), ("wind", p.wind), ("gust", p.gust),
                ("traffic", p.traffic), ("traffic_tone", p.traffic_tone), ("tyres", p.tyres), ("murmur", p.murmur), ("hum", p.hum), ("neon", p.neon),
                ("birds", p.birds), ("crickets", p.crickets), ("rain", p.rain), ("water", p.water), ("muffle", p.muffle), ("level", p.level),
            ] {
                assert!((0.0..=1.0).contains(&v), "{name} = {v} for {s:?}");
            }
            assert!(p.passby_rate >= 0.0 && p.passby_rate <= 10.0 && p.steps_rate >= 0.0 && p.steps_rate <= 10.0);
        }
    }
}
