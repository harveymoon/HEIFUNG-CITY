use serde::{Deserialize, Serialize};

/// World colours. Loaded from `assets/palette.ron` and hot-reloaded. Values are
/// linear RGB; glow colours are meant to exceed 1.0 so bloom picks them up.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Palette {
    pub sky: [f32; 3],
    pub fog: [f32; 3],
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
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            sky: [0.012, 0.010, 0.030],
            fog: [0.03, 0.02, 0.07],
            ground: [0.020, 0.018, 0.040],
            ground_variation: [0.010, 0.012, 0.025],
            hills: [0.015, 0.020, 0.045],
            asphalt: [0.030, 0.030, 0.040],
            lane_marker: [0.0, 3.5, 4.0],
            curb: [0.08, 0.08, 0.11],
            lot_vacant: [0.035, 0.030, 0.055],
            zone_colors: [
                [1.0, 0.0, 0.55],  // residential magenta
                [1.0, 0.35, 0.0],  // commercial orange
                [0.55, 1.0, 0.0],  // industrial acid
                [0.6, 0.1, 1.0],   // entertainment violet
                [0.1, 0.4, 1.0],   // civic blue
                [0.0, 1.0, 0.5],   // park green
            ],
            glow_colors: vec![
                [0.0, 4.0, 5.0],
                [5.0, 0.0, 2.5],
                [5.0, 1.5, 0.0],
                [2.5, 0.3, 5.0],
                [0.0, 5.0, 2.0],
                [5.0, 4.0, 0.0],
            ],
            window_warm: [3.0, 2.0, 0.8],
            window_cold: [0.8, 2.5, 3.5],
            citizen: [0.5, 3.0, 4.0],
            citizen_spotlight: [6.0, 1.0, 4.0],
            preview: [0.0, 2.5, 3.0],
        }
    }
}

impl Palette {
    pub fn zone_color(&self, land_use: nd_sim::LandUse) -> [f32; 3] {
        self.zone_colors[land_use.index()]
    }
}
