//! What a landscape is generated from: the preset, the climate and the
//! sliders.

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LandscapePreset {
    /// A big river with tributaries through rolling hills.
    #[default]
    RiverValley,
    /// The city on a large island, smaller islands around it, sea between.
    Islands,
    /// Sea along the south, a river running down to it.
    Coast,
    /// A river fanning into channels across a low plain to the sea.
    Delta,
    /// Low, open land with a slow river, streams and a few lakes.
    Plains,
    /// No water and a flat basin: the original map.
    Flat,
}

impl LandscapePreset {
    pub const ALL: [LandscapePreset; 6] = [
        LandscapePreset::RiverValley,
        LandscapePreset::Islands,
        LandscapePreset::Coast,
        LandscapePreset::Delta,
        LandscapePreset::Plains,
        LandscapePreset::Flat,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LandscapePreset::RiverValley => "River valley",
            LandscapePreset::Islands => "Islands",
            LandscapePreset::Coast => "Coast",
            LandscapePreset::Delta => "Delta",
            LandscapePreset::Plains => "Plains",
            LandscapePreset::Flat => "Flat",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            LandscapePreset::RiverValley => "A wide river with tributaries through rolling hills.",
            LandscapePreset::Islands => "A large island for the city, smaller ones offshore.",
            LandscapePreset::Coast => "Sea to the south and a river running down to it.",
            LandscapePreset::Delta => "A river splitting into channels across a low plain.",
            LandscapePreset::Plains => "Low open land, a slow river, streams and lakes.",
            LandscapePreset::Flat => "The original flat basin: no water.",
        }
    }

    pub fn from_label(s: &str) -> Option<LandscapePreset> {
        let s = s.to_ascii_lowercase().replace([' ', '_', '-'], "");
        Self::ALL.into_iter().find(|p| p.label().to_ascii_lowercase().replace(' ', "") == s || format!("{p:?}").to_ascii_lowercase() == s)
    }
}

/// The map's climate: the look of the ground and plants, the weather it
/// gets and how much sun, wind and hot ground it has. Old saves are Desert,
/// the dry look the game always had.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Climate {
    /// Mild, green, four seasons' worth of weather.
    Temperate,
    /// Dry, sunny, hot; the original look.
    #[default]
    Desert,
    /// Hot, humid, lush; strong sun, heavy afternoon cloud, palms.
    Tropical,
    /// Snow and ice, low sun, strong wind.
    Arctic,
    /// Iceland-style: cold, windy, many geothermal hot spots.
    Volcanic,
}

impl Climate {
    pub const ALL: [Climate; 5] = [Climate::Temperate, Climate::Desert, Climate::Tropical, Climate::Arctic, Climate::Volcanic];

    pub fn label(self) -> &'static str {
        match self {
            Climate::Temperate => "Temperate",
            Climate::Desert => "Desert",
            Climate::Tropical => "Tropical",
            Climate::Arctic => "Arctic",
            Climate::Volcanic => "Volcanic",
        }
    }

    /// One line for the New Game window.
    pub fn blurb(self) -> &'static str {
        match self {
            Climate::Temperate => "Green grass and leafy trees, rain showers, a few more streams and lakes.",
            Climate::Desert => "Dry, dusty scrub under a hard sun; the fewest streams.",
            Climate::Tropical => "Lush green ground, palms, turquoise shallows, more islands, strong sun and downpours.",
            Climate::Arctic => "Snow on the ground and roofs, frozen lakes and sea ice, conifers, snowfall.",
            Climate::Volcanic => "Iceland-style: black basalt and moss, steaming vents, many geothermal hot spots.",
        }
    }

    pub fn from_label(s: &str) -> Option<Climate> {
        let s = s.trim().to_ascii_lowercase();
        let s = match s.as_str() {
            "iceland" | "icelandic" | "geothermal" => "volcanic",
            "icy" | "ice" | "snow" | "polar" => "arctic",
            "jungle" | "lush" => "tropical",
            "dry" | "arid" => "desert",
            "green" | "mild" => "temperate",
            other => other,
        };
        Self::ALL.into_iter().find(|c| c.label().to_ascii_lowercase() == s)
    }

    /// Lowercase key for the remote API.
    pub fn key(self) -> &'static str {
        match self {
            Climate::Temperate => "temperate",
            Climate::Desert => "desert",
            Climate::Tropical => "tropical",
            Climate::Arctic => "arctic",
            Climate::Volcanic => "volcanic",
        }
    }
}

/// What the New Game window's levers set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LandscapeParams {
    pub preset: LandscapePreset,
    pub seed: u64,
    /// 0..1: river width, lakes, how far the sea reaches in.
    pub water: f32,
    /// 0..1: hills inside the basin and the height of the outer hills.
    pub relief: f32,
    /// 0..1: tributaries, streams and delta channels.
    pub branching: f32,
    /// Small islands offshore (coast, delta, islands presets).
    pub islands: u8,
    /// Lay the through highway at the start.
    pub highway: bool,
    /// Ground, plants, weather and energy resources.
    pub climate: Climate,
}

impl Default for LandscapeParams {
    fn default() -> Self {
        Self::preset(LandscapePreset::RiverValley, 7)
    }
}

impl LandscapeParams {
    /// The preset's own slider settings.
    pub fn preset(preset: LandscapePreset, seed: u64) -> Self {
        let (water, relief, branching, islands) = match preset {
            LandscapePreset::RiverValley => (0.45, 0.35, 0.35, 0),
            LandscapePreset::Islands => (0.7, 0.3, 0.2, 4),
            LandscapePreset::Coast => (0.5, 0.3, 0.25, 1),
            LandscapePreset::Delta => (0.6, 0.1, 0.7, 2),
            LandscapePreset::Plains => (0.25, 0.08, 0.4, 0),
            LandscapePreset::Flat => (0.0, 0.0, 0.0, 0),
        };
        // Every map has its highway from elsewhere to elsewhere.
        Self { preset, seed, water, relief, branching, islands, highway: true, climate: Climate::default() }
    }

    /// The old map: flat, dry, no highway. Tests that need open ground use it.
    pub fn flat() -> Self {
        Self { highway: false, ..Self::preset(LandscapePreset::Flat, 7) }
    }

    pub(super) fn clamped(&self) -> Self {
        Self { water: self.water.clamp(0.0, 1.0), relief: self.relief.clamp(0.0, 1.0), branching: self.branching.clamp(0.0, 1.0), islands: self.islands.min(8), ..self.clone() }
    }
}
