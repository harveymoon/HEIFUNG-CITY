//! The long help copy, in one place, so the HUD code reads as layout and
//! the bindings are documented where they can be checked against the input
//! systems in one glance. The Help window shows most of it.

use crate::tools::Tool;
use nd_sim::PowerPlantKind;

pub const CAMERA_WALK: &str = "Mouse to look, WASD to walk, Shift to run, Tab or Esc to return.";
pub const CAMERA_ORBIT: &str = "Right-drag rotate · middle-drag or shift-drag pan · wheel or [ ] zoom · WASD pan, Q/E turn · Tab walks the street · F6 busiest junction · F7/F8/F9 a low/middle/high tier building · F11 a power plant · F12 a park · F5 a station or yard";
pub const HOTKEYS: &str = "Space pause · - / = speed · Ctrl+Z / Ctrl+Y undo and redo · Ctrl+Enter implement · Esc clears points and the selection · Backspace drops the last point or stop · Delete removes the hovered point or stop · O zoning layer · V land value · L heat layers · K land and resource layers (farmland, oil, geothermal, best place for each industry) · P pedestrian network · H drone heat vision · G seeds the demo on an empty map";
pub const TOOL_KEYS: &str = "Tools by digit 1–9 or by letter I R Z B N T Y (Edit has no letter: Q and E turn the camera).";

pub const WINDOWS: &str = "The buttons in the top bar open and close the info windows; drag them anywhere, they remember where they were. The Inspect tool opens the inspector on a building or car and the person pane on a citizen.";

/// The one-line hint under each tool's controls.
pub const TOOL_HINTS: [(Tool, &str); 9] = [
    (Tool::Road, "Click to place points; ends dropped near a road snap onto it and make a junction (white ring). Curved roads bend through every point. One-way presets make ramps: a Ramp joining a road is a yield merge with no signals. Enter or right-click adds the road, Backspace undoes a point."),
    (Tool::Zone, "Polygon: click corners around a road, Enter or right-click to add to the project; every parcel whose centre it holds takes the use. Ctrl-click inside an existing zone to repaint it. Zones drawn over older ones take the ground."),
    (Tool::Edit, "Click a road to select it for upgrading. Drag a control point to move it. Shift-click a road or zone edge to insert a point. Delete removes the hovered point."),
    (Tool::Bulldoze, "Click a road or zone to mark it for removal."),
    (Tool::Inspect, "Click a glowing citizen, a car, or a building. Hover a parcel for a quick read-out; a selected person opens the person pane, where the star keeps them in the People window's favourites."),
    (Tool::Transit, "Click streets in order to lay stops round a loop (the side you click is the side buses serve); Enter or right-click closes the line. Click a stop to select its line, drag it to move it, Delete removes it, Backspace drops the last stop."),
    (Tool::Rail, "Nothing runs until both connectors are joined: track from its rail stub to the line (a trunk already crosses the map to the south) and a road from its street stub to the city. Unjoined stubs show red. Track is laid with the Road tool's Rail preset."),
    (Tool::District, super::districts::TOOL_HINT),
    (Tool::Buildings, "Pick a sub-category and an item, then click a lot (rail pieces go anywhere). Whatever grew there is bought out and pulled down; another civic building must be bulldozed first. Green outline: it fits; the blue ring is the coverage radius by road."),
];

pub fn tool_hint(tool: Tool) -> &'static str {
    TOOL_HINTS.iter().find(|(t, _)| *t == tool).map_or("", |(_, h)| h)
}

/// The Road tool's roundabout preset.
pub const ROAD_ROUNDABOUT: &str = "Hover to see the roundabout land: the ring, where roads are cut back and joined (dots), the road it takes up (dashed red) and the buildings that would come down (red lots). Click to stamp it into the project; it snaps onto a junction (whose signals go) or a road. Ctrl+wheel or the slider sizes it, 24 to 240 m across. The middle stays open land to zone, build or draw roads in (they join the ring), even another roundabout. Traffic goes round anticlockwise; entering cars give way to the ring, on two lanes later exits keep to the inside. Ctrl+Z undoes, Ctrl+Enter builds.";
pub const ROAD_PATH: &str = "Path: 3 m wide, for walkers and bikes only. Cars never use it; it joins streets at junctions and crosses them with a zebra.";
pub const ROAD_RAIL: &str = "Rail: single track. Ends snap to other track only, so branch off the trunk line that already crosses the map; crossing a street makes a level crossing. Stations and freight yards go on lots beside the track (Rail tool).";
pub const ZONE_BRUSH: &str = "Parcels are cut along every road already; drag over them to give them the selected use (right-drag erases to unzoned). Only parcels the use may go on take it: parks anywhere, homes on a street or path, the rest on a street. One stroke is one undo.";
pub const ZONE_MARQUEE: &str = "Drag a box: on release every parcel whose centre is inside takes the selected use (right-drag erases). Parcels that refuse the use keep theirs.";
pub const PROJECTS: &str = "Several projects can be planned side by side: the active one is edited and drawn bright, the others are drawn dim (Show others). Implement builds one at a time; where two projects change the same road or parcels, the one implemented later wins and you are warned.";
pub const EDIT_ZONE_SELECTED: &str = "Selected a zone; use the Zone tool with Ctrl-click to repaint it.";
pub const TRANSIT_DEPOT: &str = "Click a vacant lot by a street (not homes or parks). Buses park here overnight and leave at 05:30; each bus costs its upkeep per day and riders pay a fare.";
pub const PROJECT_NONE: &str = "No active project. Drawing, painting or bulldozing starts one.";
pub const SEED_EXISTING: &str = "The map already has buildings: the demo is laid out blind over the centre and implements at once (no undo).";
pub const SAVES: &str = "Save writes saves/city.ron (always the latest) and a dated slot; the newest eight slots are kept. Load on a running city asks first, then comes up paused with the tools reset.";
pub const ASSETS_RELOAD: &str = "assets/palette.ron and assets/rules/buildings.ron hot-reload while running.";

pub fn power_note(k: PowerPlantKind) -> &'static str {
    match k {
        PowerPlantKind::Coal => ". Industrial land only. Cheap and steady; the haze drags land value down for hundreds of metres.",
        PowerPlantKind::Solar => ". Any vacant lot. Clean, but only by day and dimmed by cloud.",
        PowerPlantKind::Wind => ". Any vacant lot. Clean; output follows the wind.",
        PowerPlantKind::Nuclear => ". Industrial land only. Enormous, steady, expensive, and the neighbours are nervous.",
        PowerPlantKind::Hydro => ". On a lot beside a river or stream with a current of 0.8 m/s or more; output follows the current. A weir is built across the water.",
        PowerPlantKind::Tidal => ". On the sea shore. Clean; output rises and falls with the two tides a day.",
        PowerPlantKind::Geothermal => ". Over a geothermal hot spot (yellow ground, steaming vents). Steady; output follows the heat.",
    }
}

/// One line on what a service or attraction does, for the Buildings pane.
pub fn service_note(k: nd_sim::ServiceKind) -> &'static str {
    use nd_sim::ServiceKind as K;
    match k {
        K::Police => "Patrol cars answer crimes; coverage cuts crime and arrests come quicker.",
        K::Fire => "Engines put out fires before they spread; coverage cuts fire risk.",
        K::Health => "Ambulances take the sick to its beds; coverage keeps people healthy.",
        K::School => "Workers living in its district earn more (night classes, a trade).",
        K::Dispatch => "Its staff answer 911: with a dispatcher at the desk units go out in minutes, without one calls go to the slow regional line.",
        K::Monument => "Lifts land value around it; people come to look.",
        K::Stadium => "Hosts games some evenings: crowds come, tickets pay into the treasury.",
        K::Jail => "Arrested suspects serve days here; without one they are held a few hours at a police station and let go early.",
        K::TrashDepot => "Garbage trucks (two crew each) empty the bins of every building within its radius, 06:00 to 18:00. Without one only the rich pay a private hauler; everyone else's trash overflows onto the pavement.",
        K::Vertiport => "Base for the air taxi company's drones: the rich fly between vertiports and rooftop pads, above the traffic. Noisy neighbours.",
        K::ParkingEnforcement => "Officers (two a car) patrol the streets in its radius 07:00 to 21:00: a ticket for cars over the two-hour kerb limit (residents near home have permits), the tow truck after an hour more, or at once for illegal parking. Fines go to the treasury.",
    }
}
