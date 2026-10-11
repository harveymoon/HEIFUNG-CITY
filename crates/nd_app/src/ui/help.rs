//! The long help copy, in one place, so the HUD code reads as layout and
//! the bindings are documented where they can be checked against the input
//! systems in one glance. The Help window shows most of it.

use crate::tools::Tool;
use nd_sim::PowerPlantKind;

pub const CAMERA_WALK: &str = "Mouse to look, WASD to walk, Shift to run, Tab or Esc to return.";
pub const CAMERA_ORBIT: &str = "Right-drag rotate · middle-drag or shift-drag pan · wheel or [ ] zoom ([ ] raise and lower the road while the Road tool draws) · WASD pan, Q/E turn · Tab walks the street · F6 busiest junction · F7/Shift+F8/F9 a low/middle/high tier building · F8 Flag for Claude · F11 a power plant · F12 a park · F5 a station or yard";
pub const HOTKEYS: &str = "Space pause · - / = speed · Ctrl+Z / Ctrl+Y undo and redo · Ctrl+Enter implement · J City Data · Esc clears points and the selection (or closes City Data) · Backspace drops the last point or stop · Delete removes the hovered point or stop · O zoning layer · V land value · L heat layers · K land and resource layers (farmland, oil, geothermal, best place for each industry) · M metro (lines and stations) · P pedestrian network · H drone heat vision · G seeds the demo on an empty map";
pub const TOOL_KEYS: &str = "Tools by digit 1–9 or by letter I R Z B N T Y (Edit has no letter: Q and E turn the camera). The building ones (2 Road, 3 Zone, 6 Buildings, 7 Transit, 8 Rail, 9 Districts) open the Build window at their section.";

pub const WINDOWS: &str = "City Data (top bar, or J) fills the screen with every report on the city: City, Economy, Market, People and Services, each with its sub-tabs; Esc closes it, and clicking the treasury, population or power figure in the top bar opens its page. The other buttons open the tool windows (Lines, View, Saves, Help, Dev); drag them anywhere, they remember where they were. The Inspect tool opens the inspector on a building or car and the person pane on a citizen.";

/// The one-line hint under each tool's controls.
pub const TOOL_HINTS: [(Tool, &str); 9] = [
    (Tool::Road, "Click to place points; ends dropped near a road snap onto it and make a junction (white ring). Curved roads bend through every point. One-way presets make ramps: a Ramp joining a road is a yield merge with no signals. Enter or right-click adds the road, Backspace undoes a point."),
    (Tool::Zone, "Polygon: click corners around a road, Enter or right-click to add to the project; every parcel whose centre it holds takes the use. Ctrl-click inside an existing zone to repaint it. Zones drawn over older ones take the ground."),
    (Tool::Edit, "Click a road to select it for upgrading (its card, with traffic and wear, opens beside). Hover a junction or a crosswalk and press the \"Edit intersection\" button beside it for the Intersection editor (lane arrows on the road, signals, protected left, push buttons and footbridges). Drag a control point to move it. Shift-click a road or zone edge to insert a point. Delete removes the hovered point."),
    (Tool::Bulldoze, "Click a road to mark the piece under the pointer for removal (junction to junction; the rest of the road stays), or a zone to mark it. Click one of the city's own buildings (a plant, service, depot, station entrance, city parking; outlined red) to bulldoze it at once: its ground goes back to whatever the zoning plan has there. Clean up ground (in the tool's pane) sends a crew to dig out contaminated land."),
    (Tool::Inspect, "Click a glowing citizen, a car, a delivery robot, a building, or a road (its traffic, wear and Repair now). Hover a parcel for a quick read-out; a selected person opens the person pane, where the star keeps them in City Data's People, Favourites (gold in the city) and their home and work pins select those buildings. Follow keeps the camera on them (Tab: their point of view; Esc stops); Zoom to flies there."),
    (Tool::Transit, "Click a street to start a new line: each street clicked adds the next stop in order (the side you click is the side buses serve); Shift+click inserts a stop where it belongs along the loop, as clicks do when editing a line later. Drag a stop to move it, right-click or Delete removes it, click another line's stop to stop there too. Enter, Esc or Finish editing line ends it. Click a stop when not editing to pick its line; the Lines window's Edit line edits it."),
    (Tool::Rail, "Nothing runs until both connectors are joined: track from its rail stub to the line (a trunk already crosses the map to the south) and a road from its street stub to the city. Unjoined stubs show red. Track is laid with the Build window's Rail, Track."),
    (Tool::District, super::districts::TOOL_HINT),
    (Tool::Buildings, "Pick an item in the Build window, then click any lot or unzoned ground along a road: no zoning needed (rail pieces go anywhere). On unzoned ground a lot is made for it, joining neighbouring parcels if one is too small. Its ground leaves the zone it was in and becomes city ground (Civic), so nothing grows there and the zone tools leave it alone until you bulldoze it. Whatever grew there is bought out and pulled down; another city building must be bulldozed first; parks are kept. Green outline: it fits (the panel says what it does to the zoning); the blue ring is the coverage radius by road."),
];

pub fn tool_hint(tool: Tool) -> &'static str {
    TOOL_HINTS.iter().find(|(t, _)| *t == tool).map_or("", |(_, h)| h)
}

/// [interchange] The Road tool's Interchange preset.
pub const ROAD_INTERCHANGE: &str = "A diamond interchange in two clicks: click one side of the highway, then the other, where the cross street should run. The street passes under the highway (lifted over it on a short deck), and each carriageway gets an off-ramp leaving it 150 m before and an on-ramp joining it 150 m after, meeting the street at one signalled junction per side, 55 m out. Drawn along a street already under the highway, it keeps the street and adds the ramps. The hologram shows exactly what will be built; if it would cross another road, a red pin marks the spot and says why. The lanes set are the cross street's. Ctrl+Z undoes, Ctrl+Enter builds.";

/// The Road tool's roundabout preset.
pub const ROAD_ROUNDABOUT: &str = "Hover to see the roundabout land: the ring, where roads are cut back and joined (dots), the road it takes up (dashed red) and the buildings that would come down (red lots). Click to stamp it into the project; it snaps onto a junction (whose signals go) or a road. Ctrl+wheel or the slider sizes it, 24 to 240 m across. The middle stays open land to zone, build or draw roads in (they join the ring), even another roundabout. Traffic goes round anticlockwise; entering cars give way to the ring, on two lanes later exits keep to the inside. Ctrl+Z undoes, Ctrl+Enter builds.";
pub const ROAD_PATH: &str = "Path: 3 m wide, for walkers and bikes only. Cars never use it; it joins streets at junctions and crosses them with a zebra.";
pub const ROAD_RAIL: &str = "Rail: single track. Ends snap to other track only, so branch off the trunk line that already crosses the map; crossing a street makes a level crossing. Stations and freight yards go beside the track (Build window, Rail).";
pub const ZONE_BRUSH: &str = "Parcels are cut along every road already; drag over them to give them the selected use (right-drag erases to unzoned). Only parcels the use may go on take it: parks anywhere, homes on a street or path, the rest on a street. One stroke is one undo.";
pub const ZONE_MARQUEE: &str = "Drag a box: on release every parcel whose centre is inside takes the selected use (right-drag erases). Parcels that refuse the use keep theirs.";
pub const PROJECTS: &str = "Several projects can be planned side by side: the active one is edited and drawn bright, the others are drawn dim (Show others). Implement builds one at a time; where two projects change the same road or parcels, the one implemented later wins and you are warned.";
pub const EDIT_ZONE_SELECTED: &str = "Selected a zone; use the Zone tool with Ctrl-click to repaint it.";
pub const TRANSIT_DEPOT: &str = "Click a lot or unzoned ground by a street (not a park). Buses park here overnight and leave at 05:30; each bus costs its upkeep per day and riders pay a fare.";
pub const PROJECT_NONE: &str = "No active project. Drawing, painting or bulldozing starts one.";
pub const SEED_EXISTING: &str = "The map already has buildings: the demo is laid out blind over the centre and implements at once (no undo).";
pub const SAVES: &str = "Save as writes saves/<name>.ron with a map picture of the city (saves/<name>.png) and its day and population; Save writes saves/city.ron (always the latest) and a dated slot, the newest eight kept. The title screen at startup lists the saved games newest first. Load on a running city asks first, then comes up paused with the tools reset.";
pub const ASSETS_RELOAD: &str = "assets/palette.ron and assets/rules/buildings.ron hot-reload while running.";

pub fn power_note(k: PowerPlantKind) -> &'static str {
    match k {
        PowerPlantKind::Coal => ". Cheap and steady; the haze drags land value down for hundreds of metres.",
        PowerPlantKind::Solar => ". Any lot but parks and civic. Hexagonal panels on trackers that turn to face the sun; clean, but only by day and dimmed by cloud.",
        PowerPlantKind::Wind => ". Any vacant lot. Clean; output follows the wind.",
        PowerPlantKind::Nuclear => ". Enormous, steady, expensive, and the neighbours are nervous.",
        PowerPlantKind::Hydro => ". On a lot beside a river or stream with a current of 0.8 m/s or more; output follows the current, and a bigger site holds more turbine bays (one to eight). A dam is built across the water.",
        PowerPlantKind::Tidal => ". On the sea shore. Clean; output rises and falls with the two tides a day, and a bigger site holds more turbine bays (one to eight).",
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
        K::University => "A campus: enrols the town's 18-24s and brings students to dorms, buys nearby homes for dorms, builds offices, a theatre, sports and frats nearby and claims parks as plazas. Pizza places, cafés, bookshops and bars near it do well.",
        // [attractions] the landmarks: what each is (`nd_sim::attractions::about`)
        k => nd_sim::attractions::about(k),
    }
}

/// [attractions] How a landmark is placed and what it is worth, for the
/// Buildings pane and the inspector.
pub fn landmark_rules(k: nd_sim::ServiceKind) -> String {
    use nd_sim::ServiceKind as K;
    let (r, w) = k.land_value(nd_sim::Tier::Mid);
    let site = match k {
        K::Pier => " Only on the sea shore: the site must reach the sea, with open water off it for the deck (60 to 120 m out on piles).".to_string(),
        K::Observatory => format!(" Only on high ground: the site must stand {:.0} m or more above the middle of town (the median ground height of every lot).", nd_sim::attractions::OBSERVATORY_RISE),
        _ => String::new(),
    };
    format!(
        "One per city. Needs a site of {:.0} m² or more: several plots, joined when it is put down (what grew on them is bought out).{site} Open {}; admission {}; lifts land value up to +{:.2} within {:.0} m (more when fully staffed); draws {:.0} points of tourist pull.",
        k.min_area(nd_sim::Tier::Mid),
        hours_label(k.hours()),
        if k.admission() > 0.0 { format!("{:.0}", k.admission()) } else { "free".to_string() },
        w,
        r,
        k.tourist_pull()
    )
}

/// "10:00-19:00", "all hours".
pub fn hours_label(h: nd_sim::Hours) -> String {
    if h.always() {
        "all hours".into()
    } else {
        format!("{:02}:00-{:02}:00", h.open, h.close)
    }
}
