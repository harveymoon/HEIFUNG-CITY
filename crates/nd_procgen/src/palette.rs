use serde::{Deserialize, Serialize};

/// World colours. Loaded from `assets/palette.ron` and hot-reloaded. Values are
/// linear RGB; glow colours are meant to exceed 1.0 so bloom picks them up.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
    /// Night sky, fog and light. Day values below are blended in by the clock.
    pub sky: [f32; 3],
    pub fog: [f32; 3],
    pub moon: [f32; 3],
    pub sky_day: [f32; 3],
    pub fog_day: [f32; 3],
    pub sun: [f32; 3],
    pub sun_low: [f32; 3],
    pub sky_dusk: [f32; 3],
    pub ground: [f32; 3],
    pub ground_variation: [f32; 3],
    pub hills: [f32; 3],
    pub asphalt: [f32; 3],
    pub lane_marker: [f32; 3],
    pub curb: [f32; 3],
    pub lot_vacant: [f32; 3],
    pub zone_colors: [[f32; 3]; 6],
    pub glow_colors: Vec<[f32; 3]>,
    pub window_warm: [f32; 3],
    pub window_cold: [f32; 3],
    pub citizen: [f32; 3],
    pub citizen_spotlight: [f32; 3],
    pub preview: [f32; 3],
    pub preview_remove: [f32; 3],
    pub car_colors: Vec<[f32; 3]>,
    pub headlight: [f32; 3],
    pub taillight: [f32; 3],
    pub signal_green: [f32; 3],
    pub signal_red: [f32; 3],
    pub lane_divider: [f32; 3],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            sky: [0.012, 0.010, 0.030],
            fog: [0.03, 0.02, 0.07],
            moon: [0.45, 0.55, 1.0],
            sky_day: [0.30, 0.42, 0.62],
            fog_day: [0.38, 0.45, 0.58],
            sun: [1.0, 0.95, 0.85],
            sun_low: [1.0, 0.45, 0.25],
            sky_dusk: [0.35, 0.10, 0.30],
            ground: [0.035, 0.030, 0.060],
            ground_variation: [0.015, 0.018, 0.030],
            hills: [0.040, 0.050, 0.075],
            asphalt: [0.018, 0.018, 0.026],
            lane_marker: [0.0, 2.0, 2.4],
            curb: [0.10, 0.10, 0.14],
            lot_vacant: [0.045, 0.040, 0.070],
            zone_colors: [
                [1.0, 0.0, 0.55],  // residential magenta
                [1.0, 0.35, 0.0],  // commercial orange
                [0.55, 1.0, 0.0],  // industrial acid
                [0.6, 0.1, 1.0],   // entertainment violet
                [0.1, 0.4, 1.0],   // civic blue
                [0.0, 1.0, 0.5],   // park green
            ],
            glow_colors: vec![
                [0.0, 2.6, 3.2],
                [3.2, 0.0, 1.6],
                [3.2, 1.0, 0.0],
                [1.6, 0.2, 3.2],
                [0.0, 3.2, 1.3],
                [3.2, 2.6, 0.0],
            ],
            window_warm: [1.5, 1.05, 0.45],
            window_cold: [0.45, 1.25, 1.7],
            citizen: [0.5, 3.0, 4.0],
            citizen_spotlight: [6.0, 1.0, 4.0],
            preview: [0.0, 1.6, 2.0],
            preview_remove: [2.5, 0.15, 0.4],
            car_colors: vec![
                [0.06, 0.06, 0.08],
                [0.10, 0.11, 0.15],
                [0.75, 0.78, 0.85],
                [0.85, 0.65, 0.05],
                [0.08, 0.80, 0.95],
                [0.95, 0.05, 0.55],
                [0.75, 0.05, 0.10],
                [0.35, 0.10, 0.85],
            ],
            headlight: [3.5, 3.5, 3.0],
            taillight: [3.0, 0.15, 0.2],
            signal_green: [0.1, 3.0, 0.6],
            signal_red: [3.0, 0.2, 0.3],
            lane_divider: [0.0, 0.9, 1.1],
        }
    }
}

impl Palette {
    pub fn zone_color(&self, land_use: nd_sim::LandUse) -> [f32; 3] {
        self.zone_colors[land_use.index()]
    }
}
