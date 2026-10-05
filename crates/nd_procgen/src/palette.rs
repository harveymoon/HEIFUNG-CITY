use serde::{Deserialize, Serialize};

/// Rust tone used by the building grammar for units and stains.
pub const RUST: [f32; 3] = [0.42, 0.22, 0.12];
pub const RUST_STAIN: [f32; 3] = [0.30, 0.16, 0.09];

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
    pub centre_line: [f32; 3],
    pub street_lamp: [f32; 3],
    pub lamp: [f32; 3],
    pub sign_colors: Vec<[f32; 3]>,
    pub aviation_light: [f32; 3],
    pub rust: [f32; 3],
    pub foliage: [f32; 3],
    pub overlay_low: [f32; 3],
    pub overlay_high: [f32; 3],
    pub container_colors: Vec<[f32; 3]>,
    pub tarp_colors: Vec<[f32; 3]>,
    pub led_colors: Vec<[f32; 3]>,
    pub concrete: Vec<[f32; 3]>,
    pub glass: Vec<[f32; 3]>,
    pub stone: [f32; 3],
    pub steel: [f32; 3],
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            sky: [0.020, 0.018, 0.028],
            fog: [0.05, 0.045, 0.06],
            moon: [0.5, 0.55, 0.75],
            sky_day: [0.42, 0.44, 0.46],
            fog_day: [0.48, 0.47, 0.45],
            sun: [1.0, 0.93, 0.82],
            sun_low: [1.0, 0.55, 0.3],
            sky_dusk: [0.38, 0.22, 0.18],
            ground: [0.060, 0.058, 0.052],
            ground_variation: [0.025, 0.022, 0.018],
            hills: [0.070, 0.075, 0.070],
            asphalt: [0.030, 0.030, 0.032],
            lane_marker: [0.38, 0.39, 0.36],
            curb: [0.13, 0.13, 0.13],
            lot_vacant: [0.075, 0.070, 0.060],
            zone_colors: [
                [0.95, 0.50, 0.35], // residential terracotta
                [0.95, 0.75, 0.25], // commercial amber
                [0.60, 0.65, 0.30], // industrial olive
                [0.75, 0.40, 0.80], // entertainment mauve
                [0.40, 0.60, 0.90], // civic steel blue
                [0.35, 0.80, 0.45], // park green
            ],
            glow_colors: vec![[1.3, 0.85, 0.45], [1.0, 1.05, 1.2], [1.2, 0.5, 0.35], [0.6, 0.9, 1.1]],
            window_warm: [1.2, 0.85, 0.5],
            window_cold: [0.8, 0.95, 1.15],
            citizen: [0.9, 1.4, 1.6],
            citizen_spotlight: [2.5, 1.2, 0.6],
            preview: [0.5, 1.4, 1.8],
            preview_remove: [2.0, 0.3, 0.3],
            car_colors: vec![
                [0.06, 0.06, 0.07],
                [0.14, 0.15, 0.17],
                [0.70, 0.72, 0.75],
                [0.80, 0.60, 0.10],
                [0.20, 0.30, 0.45],
                [0.50, 0.12, 0.12],
                [0.35, 0.38, 0.32],
                [0.55, 0.55, 0.58],
            ],
            headlight: [2.8, 2.8, 2.5],
            taillight: [2.2, 0.15, 0.15],
            signal_green: [0.2, 2.2, 0.5],
            signal_red: [2.4, 0.2, 0.2],
            lane_divider: [0.30, 0.31, 0.28],
            centre_line: [0.55, 0.45, 0.15],
            street_lamp: [1.6, 1.2, 0.7],
            lamp: [1.4, 1.1, 0.7],
            sign_colors: vec![[1.4, 0.9, 0.4], [1.1, 1.15, 1.3], [1.3, 0.45, 0.35], [0.6, 1.0, 1.2], [1.2, 0.5, 0.9]],
            aviation_light: [2.0, 0.2, 0.15],
            rust: RUST,
            foliage: [0.22, 0.34, 0.18],
            overlay_low: [0.1, 0.25, 0.7],
            overlay_high: [1.0, 0.6, 0.1],
            container_colors: vec![
                [0.40, 0.16, 0.12],
                [0.18, 0.26, 0.36],
                [0.20, 0.30, 0.22],
                [0.55, 0.33, 0.14],
                [0.33, 0.33, 0.33],
                [0.50, 0.48, 0.42],
                [0.38, 0.32, 0.15],
                [0.28, 0.18, 0.25],
                [0.22, 0.22, 0.24],
            ],
            tarp_colors: vec![[0.10, 0.30, 0.65], [0.40, 0.40, 0.38], [0.55, 0.30, 0.10]],
            led_colors: vec![[2.4, 0.3, 0.9], [0.3, 1.8, 2.6], [2.6, 1.8, 0.3], [0.5, 2.4, 0.8], [2.4, 0.5, 0.4], [1.8, 1.9, 2.4]],
            concrete: vec![[0.42, 0.41, 0.39], [0.36, 0.36, 0.37], [0.48, 0.46, 0.42], [0.30, 0.31, 0.32]],
            glass: vec![[0.22, 0.32, 0.45], [0.30, 0.40, 0.52], [0.18, 0.26, 0.36], [0.34, 0.42, 0.50], [0.26, 0.38, 0.42]],
            stone: [0.24, 0.23, 0.22],
            steel: [0.55, 0.57, 0.60],
        }
    }
}

impl Palette {
    pub fn zone_color(&self, land_use: nd_sim::LandUse) -> [f32; 3] {
        self.zone_colors[land_use.index()]
    }
}
