//! The tools and their modes: which tool, how the Zone tool zones, the
//! Buses tool's mode, the depth layer, the Road tool's levels.

use bevy::prelude::*;
use nd_sim::{GestureId, Vec2 as SVec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Inspect,
    Road,
    Zone,
    Edit,
    Bulldoze,
    /// Placed specials by sub-category: power, services, transit, rail,
    /// attractions (`nd_sim::services::BuildCategory`).
    Buildings,
    Transit,
    Rail,
    /// [districts] Paint named districts and set their policies
    /// (`crate::district_tool`).
    District,
}

impl Tool {
    /// Toolbar order, which is also the digit hotkey order (1 to 9).
    pub const ALL: [Tool; 9] = [Tool::Inspect, Tool::Road, Tool::Zone, Tool::Edit, Tool::Bulldoze, Tool::Buildings, Tool::Transit, Tool::Rail, Tool::District];

    /// The name on the toolbar button.
    pub fn label(self) -> &'static str {
        match self {
            Tool::Inspect => "Inspect",
            Tool::Road => "Road",
            Tool::Zone => "Zone",
            Tool::Edit => "Edit",
            Tool::Bulldoze => "Bulldoze",
            Tool::Buildings => "Buildings",
            Tool::Transit => "Buses",
            Tool::Rail => "Rail",
            Tool::District => "Districts",
        }
    }

    /// The keys that pick the tool (see `tool_hotkeys`), shown under the
    /// button. Edit has no letter: Q and E turn the camera.
    pub fn hotkey(self) -> &'static str {
        match self {
            Tool::Inspect => "1 / I",
            Tool::Road => "2 / R",
            Tool::Zone => "3 / Z",
            Tool::Edit => "4",
            Tool::Bulldoze => "5 / B",
            Tool::Buildings => "6 / N",
            Tool::Transit => "7 / T",
            Tool::Rail => "8 / Y",
            Tool::District => "9",
        }
    }
}

/// How the Zone tool zones: by polygon (every parcel whose centre is inside
/// takes the use), by brush (parcels under a circle as it is dragged), or
/// by marquee (parcels whose centre is in a dragged rectangle).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZoneMode {
    Polygon,
    #[default]
    Brush,
    Marquee,
}

impl ZoneMode {
    pub const ALL: [ZoneMode; 3] = [ZoneMode::Brush, ZoneMode::Marquee, ZoneMode::Polygon];

    pub fn label(self) -> &'static str {
        match self {
            ZoneMode::Polygon => "Polygon",
            ZoneMode::Brush => "Brush",
            ZoneMode::Marquee => "Marquee",
        }
    }

    pub fn parse(s: &str) -> Option<ZoneMode> {
        ZoneMode::ALL.into_iter().find(|m| m.label().eq_ignore_ascii_case(s.trim()))
    }
}

/// A brush stroke or marquee drag in progress: which button started it,
/// whether it erases, and the paint gesture it is growing.
#[derive(Clone, Debug)]
pub struct PaintStroke {
    pub button: MouseButton,
    pub erase: bool,
    pub gesture: Option<GestureId>,
    pub painted: usize,
    /// Parcels the brush passed over that refused the use, with a reason.
    pub refused: std::collections::BTreeMap<usize, &'static str>,
    /// Where a marquee drag started.
    pub from: Option<SVec2>,
}

/// Brush radius limits, metres.
pub const BRUSH_MIN: f32 = 4.0;
pub const BRUSH_MAX: f32 = 80.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TransitMode {
    #[default]
    Route,
    Depot,
    /// [subway] Place metro stations and join them into lines.
    Metro,
    /// [cable] Draw cable car lines station to station (`cable_ux`).
    Cable,
}

/// Which layer of the city the view and the tools work on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Depth {
    /// Tunnels: the surface hidden, ground roads as faint outlines.
    Underground,
    #[default]
    Ground,
    /// Routes in the air (a placeholder for now).
    Sky,
}

impl Depth {
    pub const ALL: [Depth; 3] = [Depth::Underground, Depth::Ground, Depth::Sky];

    pub fn label(self) -> &'static str {
        match self {
            Depth::Underground => "Underground",
            Depth::Ground => "Ground",
            Depth::Sky => "Sky",
        }
    }

    pub fn parse(s: &str) -> Option<Depth> {
        Depth::ALL.iter().copied().find(|d| d.label().eq_ignore_ascii_case(s.trim()))
    }

    /// The next layer for the cycle key.
    pub fn next(self) -> Depth {
        match self {
            Depth::Underground => Depth::Ground,
            Depth::Ground => Depth::Sky,
            Depth::Sky => Depth::Underground,
        }
    }
}

/// Elevation steps of the Road tool (Page Up / Page Down, or ] and [ while
/// drawing; Shift+[ ] for the whole road), metres.
pub const ELEVATION_STEP: f32 = 2.0;
pub const ELEVATION_MIN: f32 = -20.0;
pub const ELEVATION_MAX: f32 = 24.0;

/// How the sub-pane names a level.
pub fn elevation_label(level: f32) -> String {
    if level == 0.0 {
        "ground".into()
    } else if level >= nd_sim::roads::VIADUCT_LEVEL {
        format!("+{level:.0} m viaduct")
    } else if level > 0.0 {
        format!("+{level:.0} m embankment")
    } else if level <= nd_sim::roads::TUNNEL_LEVEL {
        format!("\u{2212}{:.0} m tunnel", -level)
    } else {
        format!("\u{2212}{:.0} m cutting", -level)
    }
}

/// [fix/roads-tools] The Road tool's circle: radius limits, metres.
pub const CIRCLE_MIN_RADIUS: f32 = 15.0;
pub const CIRCLE_MAX_RADIUS: f32 = 400.0;

/// [fix/roads-tools] The closed curve the Road tool's circle lays with its
/// centre at `center` and its edge through `edge` (radius kept in limits).
pub fn circle_road(center: SVec2, edge: SVec2) -> Vec<SVec2> {
    let r = center.distance(edge).clamp(CIRCLE_MIN_RADIUS, CIRCLE_MAX_RADIUS);
    nd_sim::plan::circle_points(center, r)
}

/// [fix/roads-tools] How the Road tool draws: point by point with straight
/// legs, a smooth curve through the points, or a circle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawMethod {
    Straight,
    Curved,
    Circle,
}

impl DrawMethod {
    pub const ALL: [DrawMethod; 3] = [DrawMethod::Straight, DrawMethod::Curved, DrawMethod::Circle];

    pub fn label(self) -> &'static str {
        match self {
            DrawMethod::Straight => "Straight",
            DrawMethod::Curved => "Curved",
            DrawMethod::Circle => "Circle",
        }
    }

    pub fn parse(s: &str) -> Option<DrawMethod> {
        DrawMethod::ALL.into_iter().find(|m| m.label().eq_ignore_ascii_case(s.trim()))
    }
}

/// [fix/roads-tools] The road types of the Road tool, in menu order: each
/// has its own lanes (`ToolState::pick_type`). The old presets (Lane,
/// Avenue, Ramp, Tunnel...) are a type with lanes or a level set, still
/// taken by the remote API's `preset`.
pub const ROAD_TYPES: [&str; 4] = ["Path", "Street", "On-ramp", "Highway"];
