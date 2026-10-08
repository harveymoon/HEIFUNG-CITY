//! The Build window: everything the player can put down, in one movable,
//! resizable window. On the left a tree of sections (Roads, Zoning, Parks,
//! Districts, Power, Services, Transit, Rail, Water, Attractions), some
//! with sub-sections; in the middle a grid of thumbnails of the section's
//! items (buildings rendered from the procgen generator, see
//! `render/thumbs.rs`; roads and zones as icons); on the right what the
//! hovered or picked item costs and does, then the active tool's options.
//!
//! Picking an item picks the tool that places it with the right preset:
//! the tools themselves (`crate::tools::Tool`) are unchanged, so their
//! hotkeys and the remote API's `tool` command work as before, and the
//! window follows them: switching to a building tool by key opens it at
//! that tool's section.

use bevy_egui::egui::{self, Color32, Rect};
use nd_sim::roads::RoadKind;
use nd_sim::{BuildCategory, LandUse, Placeable, PortKind, RailFacility, ServiceKind, Tier};

use super::{help, hint, icons, pane, section, zone_color32, UiCtx, GOLD, IVORY};
use crate::tools::{Tool, ToolState, TransitMode, ROAD_PRESETS};

/// The road presets offered under Roads, in menu order (Rail track is
/// under Rail).
const ROADS: [&str; 10] = ["Path", "Lane", "Street", "Avenue", "One-way", "Ramp", "Highway", "Interchange", "Tunnel", "Roundabout"];

/// A section of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cat {
    Roads,
    Zoning,
    Parks,
    Districts,
    Power,
    Services,
    Transit,
    Rail,
    Water,
    Attractions,
}

impl Cat {
    pub const ALL: [Cat; 10] = [Cat::Roads, Cat::Zoning, Cat::Parks, Cat::Districts, Cat::Power, Cat::Services, Cat::Transit, Cat::Rail, Cat::Water, Cat::Attractions];

    pub fn label(self) -> &'static str {
        match self {
            Cat::Roads => "Roads",
            Cat::Zoning => "Zoning",
            Cat::Parks => "Parks",
            Cat::Districts => "Districts",
            Cat::Power => "Power",
            Cat::Services => "Services",
            Cat::Transit => "Transit",
            Cat::Rail => "Rail",
            Cat::Water => "Water",
            Cat::Attractions => "Attractions",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Cat::Roads => "roads",
            Cat::Zoning => "zoning",
            Cat::Parks => "parks",
            Cat::Districts => "districts",
            Cat::Power => "power",
            Cat::Services => "services",
            Cat::Transit => "transit",
            Cat::Rail => "rail",
            Cat::Water => "water",
            Cat::Attractions => "attractions",
        }
    }

    pub fn parse(s: &str) -> Option<Cat> {
        Cat::ALL.into_iter().find(|c| c.key().eq_ignore_ascii_case(s.trim()) || c.label().eq_ignore_ascii_case(s.trim()))
    }
}

/// One thing in the grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Item {
    /// A Road tool preset by label (`ROADS`).
    Road(&'static str),
    /// Rail track: the Road tool's Rail preset.
    Track,
    Zone(LandUse),
    /// [waterfront] Paint public beaches on parcels that run down to the
    /// water (the Zone tool, a park sub-zone).
    Beach,
    /// Paint districts (the District tool).
    District,
    /// Anything the Buildings tool places (services at their middle tier;
    /// the tier is an option).
    Place(Placeable),
    /// A station, the Central Station or a freight yard (the Rail tool, which
    /// shows the track and street sides it would fuse to).
    RailPiece(RailFacility),
    /// Draw bus lines (the Transit tool).
    BusLines,
    /// Lay metro lines between stations (the Transit tool's Metro mode).
    MetroLines,
}

impl Item {
    pub fn label(self) -> String {
        match self {
            Item::Road(l) => l.to_string(),
            Item::Track => "Rail track".into(),
            Item::Zone(LandUse::Park) => "Park".into(),
            Item::Beach => "Public beach".into(),
            Item::Zone(lu) => lu.label().to_string(),
            Item::District => "Paint districts".into(),
            // [plant-groups] Solar and wind are sized to the lot: no flat MW
            // (the tile's price line and the figures quote them per m²).
            Item::Place(Placeable::Plant(k)) => match k.reference_area() {
                // [plant-scale] Coal, nuclear, geothermal: sized to the site.
                Some(_) if k.sized_when_built() => {
                    let (lo, hi) = k.size_range();
                    format!("{} {:.0}–{:.0} MW", k.label(), k.capacity_at(lo), k.capacity_at(hi))
                }
                Some(_) => k.label().to_string(),
                None => format!("{} {:.0} MW", k.label(), k.capacity_mw()),
            },
            Item::Place(p) => p.menu_label().to_string(),
            Item::RailPiece(f) => Placeable::Rail(f).menu_label().to_string(),
            Item::BusLines => "Bus lines".into(),
            Item::MetroLines => "Metro lines".into(),
        }
    }

    /// The building it puts down, for its thumbnail and figures.
    pub fn placeable(self) -> Option<Placeable> {
        match self {
            Item::Place(p) => Some(p),
            Item::RailPiece(f) => Some(Placeable::Rail(f)),
            _ => None,
        }
    }

    /// Same item, the tier aside.
    fn same(self, other: Item) -> bool {
        match (self, other) {
            (Item::Place(a), Item::Place(b)) => a.with_tier(Tier::Mid) == b.with_tier(Tier::Mid),
            (a, b) => a == b,
        }
    }
}

/// A sub-section: its heading and items.
pub struct Group {
    pub name: &'static str,
    pub items: Vec<Item>,
}

/// The services, by sub-section.
fn service_group(k: ServiceKind) -> &'static str {
    match k {
        ServiceKind::Police | ServiceKind::Fire | ServiceKind::Dispatch | ServiceKind::Jail | ServiceKind::ParkingEnforcement => "Safety",
        ServiceKind::Health | ServiceKind::School | ServiceKind::University => "Health and education",
        ServiceKind::TrashDepot => "Sanitation",
        _ => "Other",
    }
}

/// The transit placeables, by sub-section.
fn transit_group(p: Placeable) -> &'static str {
    match p {
        Placeable::BusDepot => "Buses",
        Placeable::MetroStation => "Metro",
        Placeable::Service(ServiceKind::Vertiport, _) => "Air",
        Placeable::ParkingLot(_) => "Parking",
        Placeable::Toll(_) => "Tolls",
        Placeable::CargoHub => "Freight",
        _ => "Other",
    }
}

/// Where a sim build category's items go. New placeables in the sim's
/// categories show up here without UI changes.
#[allow(unreachable_patterns)]
fn cat_of_build(c: BuildCategory) -> Cat {
    match c {
        BuildCategory::Power => Cat::Power,
        BuildCategory::Services => Cat::Services,
        BuildCategory::Transit => Cat::Transit,
        BuildCategory::Rail => Cat::Rail,
        BuildCategory::Water => Cat::Water,
        BuildCategory::Attractions => Cat::Attractions,
        _ => Cat::Services,
    }
}

/// Every placeable the sim offers, with the section it is shown in: the
/// cargo hub moves from the sim's Rail category to Transit (it serves
/// lorries as much as trains), rail pieces become Rail tool items.
fn placeables() -> Vec<(Cat, Item)> {
    let mut out = Vec::new();
    for bc in BuildCategory::ALL {
        for p in bc.items() {
            let item = match p {
                Placeable::Rail(f) => Item::RailPiece(f),
                other => Item::Place(other),
            };
            let cat = match p {
                Placeable::CargoHub => Cat::Transit,
                _ => cat_of_build(bc),
            };
            out.push((cat, item));
        }
    }
    out
}

/// Every placeable the Build window offers (for the thumbnails).
pub fn all_placeables() -> Vec<Placeable> {
    placeables().into_iter().filter_map(|(_, i)| i.placeable()).collect()
}

/// A section's sub-sections and items, in menu order.
pub fn groups(cat: Cat) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut add = |name: &'static str, item: Item| match groups.iter_mut().find(|g| g.name == name) {
        Some(g) => g.items.push(item),
        None => groups.push(Group { name, items: vec![item] }),
    };
    match cat {
        Cat::Roads => {
            for r in ROADS {
                let group = match r {
                    "Path" => "Paths",
                    "Highway" | "Ramp" | "Interchange" => "Highways",
                    "Tunnel" | "Roundabout" => "Special",
                    _ => "Streets",
                };
                add(group, Item::Road(r));
            }
        }
        Cat::Zoning => {
            for lu in LandUse::ALL.into_iter().filter(|l| *l != LandUse::Park) {
                add("Land use", Item::Zone(lu));
            }
        }
        Cat::Parks => {
            add("Parks", Item::Zone(LandUse::Park));
            add("Parks", Item::Beach);
        }
        Cat::Districts => add("Districts", Item::District),
        Cat::Transit => {
            add("Buses", Item::BusLines);
            add("Metro", Item::MetroLines);
        }
        Cat::Rail => add("Track", Item::Track),
        _ => {}
    }
    for (c, item) in placeables() {
        if c != cat {
            continue;
        }
        let name = match (cat, item) {
            (Cat::Services, Item::Place(Placeable::Service(k, _))) => service_group(k),
            (Cat::Transit, Item::Place(p)) => transit_group(p),
            (Cat::Rail, _) => "Stations and yards",
            (Cat::Power, _) => "Power",
            // [economy] the waterfront's marinas and fishing ports
            (Cat::Water, Item::Place(Placeable::Dock(k))) if k.is_marina() => "Marinas",
            (Cat::Water, Item::Place(Placeable::Dock(k))) if k.is_fishing() => "Fishing",
            (Cat::Water, _) => "Ports and ferries",
            (Cat::Attractions, _) => "Attractions",
            _ => "Other",
        };
        add(name, item);
    }
    groups
}

/// What the active tool would put down now, as a grid item.
#[allow(unreachable_patterns)]
pub fn current_item(t: &ToolState) -> Option<Item> {
    match t.tool {
        Tool::Road => Some(if t.roundabout {
            Item::Road("Roundabout")
        } else if t.interchange {
            Item::Road("Interchange")
        } else if t.tunnel {
            Item::Road("Tunnel")
        } else {
            match t.kind {
                RoadKind::Rail => Item::Track,
                RoadKind::Path => Item::Road("Path"),
                RoadKind::Street => {
                    let label = ROAD_PRESETS.iter().find(|(_, f, b, k)| *k == RoadKind::Street && *f == t.lanes_fwd && *b == t.lanes_bwd).map_or("Street", |p| p.0);
                    Item::Road(ROADS.iter().copied().find(|r| *r == label).unwrap_or("Street"))
                }
            }
        }),
        Tool::Zone => Some(if t.beach && t.land_use == LandUse::Park { Item::Beach } else { Item::Zone(t.land_use) }),
        Tool::District => Some(Item::District),
        Tool::Buildings => Some(match t.build {
            Placeable::Rail(f) => Item::RailPiece(f),
            p => Item::Place(p.with_tier(Tier::Mid)),
        }),
        Tool::Rail => Some(Item::RailPiece(t.rail_kind)),
        Tool::Transit => Some(match t.transit_mode {
            TransitMode::Route => Item::BusLines,
            TransitMode::Metro => Item::MetroLines,
            TransitMode::Depot => Item::Place(Placeable::BusDepot),
        }),
        _ => None,
    }
}

/// The section an item is listed in.
pub fn cat_of(item: Item) -> Cat {
    match item {
        Item::Road(_) => Cat::Roads,
        Item::Track | Item::RailPiece(_) => Cat::Rail,
        Item::Zone(LandUse::Park) => Cat::Parks,
        Item::Beach => Cat::Parks,
        Item::Zone(_) => Cat::Zoning,
        Item::District => Cat::Districts,
        Item::BusLines | Item::MetroLines => Cat::Transit,
        Item::Place(p) => placeables().into_iter().find(|(_, i)| i.same(Item::Place(p))).map_or(Cat::Services, |(c, _)| c),
    }
}

/// Pick the tool and preset that put `item` down.
pub fn activate(item: Item, t: &mut ToolState) {
    let tool = match item {
        Item::Road(_) | Item::Track => Tool::Road,
        Item::Zone(_) | Item::Beach => Tool::Zone,
        Item::District => Tool::District,
        Item::Place(_) => Tool::Buildings,
        Item::RailPiece(_) => Tool::Rail,
        Item::BusLines | Item::MetroLines => Tool::Transit,
    };
    if t.tool != tool {
        t.tool = tool;
        t.points.clear();
        t.drag = None;
        t.stroke = None;
        t.ring_pin = None;
    }
    match item {
        Item::Road("Roundabout") => {
            t.roundabout = true;
            t.interchange = false;
            t.tunnel = false;
            t.points.clear();
        }
        Item::Road(label) => {
            t.pick_preset(label);
        }
        Item::Track => {
            t.pick_preset("Rail");
        }
        Item::Zone(lu) => {
            t.land_use = lu;
            t.unzone = false;
            t.beach = false;
        }
        Item::Beach => {
            t.land_use = LandUse::Park;
            t.unzone = false;
            t.beach = true;
        }
        Item::District => {}
        Item::Place(p) => {
            // Keep the tier the player chose; the grid shows middle tiers.
            t.build_cat = p.category();
            t.build = p;
        }
        Item::RailPiece(f) => {
            t.rail_kind = f;
            t.dock = None;
        }
        Item::BusLines | Item::MetroLines => {
            let mode = if item == Item::BusLines { TransitMode::Route } else { TransitMode::Metro };
            if t.transit_mode != mode {
                t.transit_mode = mode;
                t.route_sel = None;
                t.metro_draft.clear();
            }
        }
    }
}

/// Whether a tool is one the Build window picks (and shows options for).
/// Any other tool gets a quick button on the toolbar.
pub fn is_build_tool(tool: Tool) -> bool {
    matches!(tool, Tool::Road | Tool::Zone | Tool::Buildings | Tool::Transit | Tool::Rail | Tool::District)
}

// ── [build-highlight] what the map shows ────────────────────────────────────

/// The kind a building is matched by: a service at its middle tier (a
/// clinic and a hospital are both Health), anything else as it is.
pub fn norm(p: Placeable) -> Placeable {
    p.with_tier(Tier::Mid)
}

/// The kinds of building a section (or sub-section) lists, the ones already
/// on the map get highlighted: everything that stands on a lot or by the
/// track. Roads, zones, districts, lines and toll booths have none.
pub fn highlight_kinds(cat: Cat, sub: Option<&str>) -> Vec<Placeable> {
    groups(cat)
        .into_iter()
        .filter(|g| sub.is_none_or(|s| s == g.name))
        .flat_map(|g| g.items)
        .filter_map(|i| i.placeable())
        .filter(|p| !matches!(p, Placeable::Toll(_)))
        .map(norm)
        .collect()
}

/// What the Build window asks the map to highlight: the section's kinds,
/// the picked item (if it is one of them) strongly.
pub fn highlight_sel(cat: Option<Cat>, sub: Option<&'static str>, tools: &ToolState) -> Option<crate::build_highlight::HighlightSel> {
    let cat = cat?;
    let kinds = highlight_kinds(cat, sub);
    if kinds.is_empty() {
        return None;
    }
    let focus = if is_build_tool(tools.tool) { current_item(tools).and_then(|i| i.placeable()).map(norm).filter(|p| kinds.contains(p)) } else { None };
    Some(crate::build_highlight::HighlightSel { cat, sub, focus })
}

/// The remote API's `build_highlight {category}`: a section key ("power"),
/// a sub-section ("safety", "parking") or a kind ("health", "police",
/// "metro_station"), as the section, sub-section and item to show.
pub fn resolve_highlight(name: &str) -> Option<(Cat, Option<&'static str>, Option<Item>)> {
    let n = name.trim().to_ascii_lowercase().replace(['_', '-'], " ");
    if let Some(cat) = Cat::parse(&n) {
        return Some((cat, None, None));
    }
    let with_kinds = |f: &dyn Fn(&Group) -> bool| -> Option<(Cat, &'static str)> {
        Cat::ALL.into_iter().find_map(|c| groups(c).into_iter().find(|g| f(g) && !highlight_kinds(c, Some(g.name)).is_empty()).map(|g| (c, g.name)))
    };
    if let Some((cat, g)) = with_kinds(&|g: &Group| g.name.eq_ignore_ascii_case(&n)) {
        return Some((cat, Some(g), None));
    }
    if let Some(p) = Placeable::parse(&n.replace(' ', "_"), None).filter(|p| !matches!(p, Placeable::Toll(_))) {
        for cat in Cat::ALL {
            for g in groups(cat) {
                if let Some(item) = g.items.iter().copied().find(|i| i.placeable().map(norm) == Some(norm(p))) {
                    return Some((cat, Some(g.name), Some(item)));
                }
            }
        }
    }
    with_kinds(&|g: &Group| g.name.to_ascii_lowercase().split([' ', ',']).any(|w| w == n)).map(|(c, g)| (c, Some(g), None))
}

// ── window state ────────────────────────────────────────────────────────────

fn open_id() -> egui::Id {
    egui::Id::new("build_window_open")
}

pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id()).unwrap_or(false))
}

pub fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

/// Close the window; a building tool in hand goes back to Inspect, so
/// nothing is left half-picked with its options out of sight.
pub fn close(ctx: &egui::Context, tools: &mut ToolState) {
    set_open(ctx, false);
    if is_build_tool(tools.tool) {
        tools.tool = Tool::Inspect;
        tools.points.clear();
        tools.drag = None;
    }
}

/// What the window remembers between frames (egui memory).
#[derive(Clone, Default)]
struct State {
    cat: Option<Cat>,
    /// The sub-section the grid is narrowed to, if any.
    sub: Option<&'static str>,
    /// The item the active tool put down last frame, to notice a change made
    /// elsewhere (a hotkey, the API) and follow it.
    last: Option<Item>,
    /// The tool last frame, to open the window when a building tool is
    /// picked elsewhere.
    last_tool: Option<Tool>,
}

fn state_id() -> egui::Id {
    egui::Id::new("build_window_state")
}

/// Open the window on a section (the remote API's `window {name: build, section}`).
pub fn show_section(ctx: &egui::Context, cat: Cat) {
    let mut st: State = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();
    st.cat = Some(cat);
    st.sub = None;
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
    set_open(ctx, true);
}

/// [build-highlight] Open the window on a section and one of its
/// sub-sections (None for all of it).
pub fn show_sub(ctx: &egui::Context, cat: Cat, sub: Option<&'static str>) {
    let mut st: State = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();
    st.cat = Some(cat);
    st.sub = sub;
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
    set_open(ctx, true);
}

/// The section the window shows (for `status`).
pub fn section_of(ctx: &egui::Context) -> Option<Cat> {
    ctx.data(|d| d.get_temp::<State>(state_id())).and_then(|s| s.cat)
}

/// Follow the tools: a building tool picked by key or by the API opens the
/// window at its section. Called every frame, open or not.
pub fn follow_tools(ctx: &egui::Context, tools: &ToolState) {
    let mut st: State = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();
    let now = current_item(tools);
    if st.last_tool != Some(tools.tool) && st.last_tool.is_some() {
        // A building tool picked elsewhere opens the window; any other tool
        // (Inspect, Edit, Bulldoze: a button, a key or the API) closes it.
        set_open(ctx, is_build_tool(tools.tool));
    }
    if let Some(item) = now {
        if st.last.is_none_or(|l| !l.same(item)) || st.cat.is_none() {
            let cat = cat_of(item);
            if st.cat != Some(cat) {
                st.sub = None;
            }
            st.cat = Some(cat);
        }
    }
    st.last = now;
    st.last_tool = Some(tools.tool);
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
}

// ── the window ──────────────────────────────────────────────────────────────

/// Tile size in the grid.
const TILE: egui::Vec2 = egui::vec2(104.0, 140.0);
const THUMB: f32 = 96.0;
const TREE_WIDTH: f32 = 172.0;
const INFO_WIDTH: f32 = 330.0;

pub fn window(ctx: &egui::Context, p: &mut UiCtx, content: Rect) {
    follow_tools(ctx, p.tools);
    if !is_open(ctx) {
        p.highlight.sel = None;
        return;
    }
    let mut st: State = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();
    let mut keep = true;
    let size = egui::vec2(940.0f32.min(content.width() - 24.0), 440.0f32.min(content.height() - 24.0));
    egui::Window::new("Build")
        .id(egui::Id::new("build_window"))
        .title_bar(false)
        .resizable(true)
        .constrain_to(content)
        .default_pos(egui::pos2(content.left() + 12.0, content.bottom() - size.y - 12.0))
        .default_size(size)
        .min_size(egui::vec2(560.0, 260.0))
        .show(ctx, |ui| {
            keep = pane::title_bar(ui, "Build", None);
            // Three columns in fixed rectangles, so a wide slider or a long
            // hint in the options never widens the window.
            let full = ui.available_rect_before_wrap();
            ui.allocate_rect(full, egui::Sense::hover());
            let info_w = INFO_WIDTH.min(full.width() * 0.4);
            let tree_r = Rect::from_min_size(full.min, egui::vec2(TREE_WIDTH, full.height()));
            let info_r = Rect::from_min_max(egui::pos2(full.right() - info_w, full.top()), full.max);
            let grid_r = Rect::from_min_max(egui::pos2(tree_r.right() + 8.0, full.top()), egui::pos2(info_r.left() - 8.0, full.bottom()));
            let line = egui::Stroke::new(1.0, Color32::from_rgb(50, 50, 62));
            ui.painter().vline(tree_r.right() + 4.0, full.y_range(), line);
            ui.painter().vline(info_r.left() - 4.0, full.y_range(), line);
            let mut hovered: Option<Item> = None;
            let column = |ui: &mut egui::Ui, r: Rect, salt: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
                // A child the window does not size itself by: whatever
                // overflows is clipped, never grows the window.
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)));
                child.set_clip_rect(r.intersect(ui.clip_rect()));
                egui::ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]).max_height(r.height()).show(&mut child, |ui| {
                    ui.set_max_width(r.width() - 10.0);
                    add(ui);
                });
            };
            column(ui, tree_r, "build_tree", &mut |ui| tree(ui, &mut st));
            column(ui, grid_r, "build_grid", &mut |ui| hovered = grid(ui, p, &st));
            column(ui, info_r, "build_info", &mut |ui| {
                // [build-highlight] What the section has on the map.
                on_the_map(ui, p);
                let shown = hovered.or(current_item(p.tools));
                if let Some(item) = shown {
                    info(ui, p, item, hovered.is_some());
                }
                if is_build_tool(p.tools.tool) {
                    ui.add_space(6.0);
                    section(ui, "OPTIONS");
                    super::tools::sub_pane(ui, p.sim, p.tools, p.plan, p.assets);
                    if !p.tools.status.is_empty() {
                        ui.label(egui::RichText::new(&p.tools.status).small().color(IVORY));
                    }
                } else {
                    hint(ui, "Pick something in the grid to build it.");
                }
            });
        });
    // [build-highlight] The section's buildings on the map.
    p.highlight.sel = if keep { highlight_sel(st.cat, st.sub, p.tools) } else { None };
    // Follow a pick made in the grid this frame.
    st.last = current_item(p.tools);
    st.last_tool = Some(p.tools.tool);
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
    if !keep {
        close(ctx, p.tools);
    }
}

/// [build-highlight] The highlighted buildings in a line or two ("12
/// hospitals · 84% of homes covered · 3 districts uncovered"), the colour
/// key, and the uncovered places as buttons that fly the camera there.
fn on_the_map(ui: &mut egui::Ui, p: &mut UiCtx) {
    let Some(view) = p.highlight.view.clone() else { return };
    if p.highlight.sel.is_none() {
        return;
    }
    let s = &view.summary;
    section(ui, "ON THE MAP");
    ui.label(egui::RichText::new(&s.headline).small().color(IVORY));
    for line in &s.detail {
        ui.label(egui::RichText::new(line).small().weak());
    }
    if !s.legend.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (col, text) in &s.legend {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, *col);
                ui.label(egui::RichText::new(text).small().weak());
                ui.add_space(4.0);
            }
        });
    }
    if !s.spots.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
            ui.label(egui::RichText::new("uncovered:").small().weak());
            for spot in s.spots.iter().take(6) {
                let b = egui::Button::new(egui::RichText::new(format!("{} ({})", spot.name, spot.homes)).small().color(Color32::from_rgb(255, 140, 120))).frame(false);
                if ui.add(b).on_hover_text("Fly there").clicked() {
                    p.fly_to = Some(bevy::math::Vec2::new(spot.at.x, spot.at.y));
                }
            }
        });
    }
    ui.add_space(4.0);
}

/// How far a sub-section sits in under its section in the tree.
const SUB_INDENT: f32 = 16.0;

fn tree(ui: &mut egui::Ui, st: &mut State) {
    for cat in Cat::ALL {
        let on = st.cat == Some(cat);
        let text = egui::RichText::new(cat.label()).strong();
        // Left-aligned rows the width of the tree: a grow atom after the
        // label keeps it at the left edge (add_sized would centre it).
        let main = egui::Button::selectable(on && st.sub.is_none(), if on { text.color(GOLD) } else { text }).right_text("").min_size(egui::vec2(TREE_WIDTH - 14.0, 26.0));
        if ui.add(main).clicked() {
            st.cat = Some(cat);
            st.sub = None;
        }
        if on {
            let groups = groups(cat);
            if groups.len() > 1 {
                for g in &groups {
                    ui.horizontal(|ui| {
                        ui.add_space(SUB_INDENT);
                        let sub = egui::Button::selectable(st.sub == Some(g.name), egui::RichText::new(g.name).small()).right_text("").min_size(egui::vec2(TREE_WIDTH - 14.0 - SUB_INDENT, 22.0));
                        if ui.add(sub).clicked() {
                            st.sub = if st.sub == Some(g.name) { None } else { Some(g.name) };
                        }
                    });
                }
            }
        }
    }
}

/// The tiles of the section (or sub-section). Returns the hovered item.
fn grid(ui: &mut egui::Ui, p: &mut UiCtx, st: &State) -> Option<Item> {
    let Some(cat) = st.cat else {
        hint(ui, "Pick a section on the left.");
        return None;
    };
    let current = current_item(p.tools);
    let mut hovered = None;
    let mut picked = None;
    for g in groups(cat).into_iter().filter(|g| st.sub.is_none_or(|s| s == g.name)) {
        section(ui, &g.name.to_uppercase());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for item in g.items {
                let on = current.is_some_and(|c| c.same(item));
                let resp = tile(ui, p, item, on);
                if resp.hovered() {
                    hovered = Some(item);
                }
                if resp.clicked() {
                    picked = Some(item);
                }
            }
        });
    }
    if let Some(item) = picked {
        activate(item, p.tools);
    }
    hovered
}

/// One tile: the thumbnail (or icon) and the name with its price.
fn tile(ui: &mut egui::Ui, p: &UiCtx, item: Item, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(TILE, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let fill = if on {
        Color32::from_rgb(110, 78, 32)
    } else if resp.hovered() {
        Color32::from_rgb(52, 44, 34)
    } else {
        Color32::from_rgb(22, 24, 36)
    };
    painter.rect_filled(rect, 4.0, fill);
    painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, if on { GOLD } else { Color32::from_rgb(60, 60, 72) }), egui::StrokeKind::Inside);
    let img = Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + 4.0 + THUMB * 0.5), egui::vec2(THUMB, THUMB));
    let thumb = item.placeable().and_then(|pl| p.thumbs.get(pl));
    match thumb {
        Some(id) => {
            painter.image(id, img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.rect_filled(img, 3.0, Color32::from_rgb(14, 16, 26));
            icon(&painter, img.shrink(18.0), item, p, if on { Color32::WHITE } else { GOLD });
        }
    }
    let name = item.label();
    // [plant-groups] Solar and wind: price and output per m² of lot.
    let price = item.placeable().map(|pl| match pl {
        Placeable::Plant(k) if k.reference_area().is_some() => {
            let a = k.reference_area().unwrap_or(1.0);
            format!("{:.1}/m² · {:.0} kW/m²", pl.cost() / a, k.capacity_mw() * 1000.0 / a)
        }
        _ => format!("{:.0}", pl.cost()),
    });
    let font = crate::ui::theme::small();
    painter.text(egui::pos2(rect.center().x, img.bottom() + 3.0), egui::Align2::CENTER_TOP, elide(&name, 14), font.clone(), if on { Color32::WHITE } else { IVORY });
    if let Some(price) = price {
        painter.text(egui::pos2(rect.center().x, img.bottom() + 20.0), egui::Align2::CENTER_TOP, format!("$ {price}"), crate::ui::theme::small(), Color32::from_rgb(170, 160, 140));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn elide(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>())
    }
}

/// An icon for items with no building to show (and while thumbnails render).
fn icon(painter: &egui::Painter, r: Rect, item: Item, p: &UiCtx, col: Color32) {
    match item {
        Item::Road("Roundabout") => icons::roundabout(painter, r, col),
        Item::Road("Interchange") => icons::interchange(painter, r, col),
        Item::Road("Tunnel") => {
            icons::road(painter, r, RoadKind::Street, 2, 2, col);
            painter.rect_stroke(r.shrink(r.width() * 0.08), r.width() * 0.3, egui::Stroke::new(1.5, Color32::from_rgb(170, 160, 150)), egui::StrokeKind::Inside);
        }
        Item::Road(label) => {
            if let Some((_, f, b, k)) = ROAD_PRESETS.iter().find(|x| x.0 == label) {
                icons::road(painter, r, *k, *f, *b, col);
            }
        }
        Item::Track => icons::road(painter, r, RoadKind::Rail, 0, 0, col),
        Item::Zone(lu) => icons::land_use(painter, r, lu, zone_color32(p.assets.palette.zone_color(lu))),
        Item::Beach => {
            // Sand down to a blue sea, a red and white umbrella.
            let sand = Color32::from_rgb(222, 196, 140);
            let sea = Color32::from_rgb(40, 150, 210);
            let inner = r.shrink(r.width() * 0.12);
            painter.rect_filled(inner, 4.0, sand);
            let water = egui::Rect::from_min_max(egui::pos2(inner.min.x, inner.center().y + inner.height() * 0.15), inner.max);
            painter.rect_filled(water, 4.0, sea);
            let top = egui::pos2(inner.center().x - inner.width() * 0.1, inner.min.y + inner.height() * 0.2);
            painter.line_segment([top, egui::pos2(top.x, inner.center().y + inner.height() * 0.1)], egui::Stroke::new(1.5, Color32::from_gray(60)));
            painter.circle_filled(top, inner.width() * 0.2, Color32::from_rgb(230, 70, 60));
        }
        Item::District => {
            let c = Color32::from_rgb(220, 90, 200);
            painter.rect_stroke(r.shrink(4.0), 6.0, egui::Stroke::new(2.0, c), egui::StrokeKind::Inside);
            painter.rect_filled(r.shrink(10.0), 4.0, c.gamma_multiply(0.35));
        }
        Item::BusLines | Item::MetroLines => {
            let c = if item == Item::BusLines { Color32::from_rgb(110, 200, 220) } else { Color32::from_rgb(190, 130, 230) };
            let pts = [r.left_bottom() + egui::vec2(4.0, -6.0), r.center() + egui::vec2(-6.0, 6.0), r.center() + egui::vec2(8.0, -8.0), r.right_top() + egui::vec2(-4.0, 6.0)];
            painter.line(pts.to_vec(), egui::Stroke::new(3.0, c));
            for q in pts {
                painter.circle_filled(q, 4.0, Color32::WHITE);
            }
        }
        Item::Place(Placeable::Toll(_)) => {
            let y = r.center().y;
            painter.line_segment([egui::pos2(r.left(), y), egui::pos2(r.right(), y)], egui::Stroke::new(r.height() * 0.45, Color32::from_rgb(60, 62, 72)));
            for i in 0..3 {
                let x = r.left() + r.width() * (0.25 + 0.25 * i as f32);
                painter.rect_filled(Rect::from_center_size(egui::pos2(x, y), egui::vec2(5.0, 12.0)), 1.0, col);
            }
            painter.line_segment([egui::pos2(r.left() + 4.0, y - 10.0), egui::pos2(r.right() - 4.0, y - 10.0)], egui::Stroke::new(2.0, Color32::from_rgb(240, 200, 80)));
        }
        // A building whose thumbnail is still rendering.
        Item::Place(_) | Item::RailPiece(_) => {
            painter.rect_stroke(r.shrink(6.0), 2.0, egui::Stroke::new(1.0, col.gamma_multiply(0.5)), egui::StrokeKind::Inside);
            painter.text(r.center(), egui::Align2::CENTER_CENTER, "…", crate::ui::theme::heading(), col);
        }
    }
}

/// What an item costs and does: the hovered one, else the one picked.
fn info(ui: &mut egui::Ui, p: &mut UiCtx, item: Item, hovered: bool) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(item.label()).color(GOLD).strong().font(crate::ui::theme::lead()));
        if !hovered {
            ui.label(egui::RichText::new("· picked").small().weak());
        }
    });
    match item {
        Item::Place(pl) => {
            let picked = current_item(p.tools).is_some_and(|c| c.same(item)) && p.tools.tool == Tool::Buildings;
            let shown = if picked { pl.with_tier(p.tools.build_tier) } else { pl };
            figures(ui, shown);
            // The picked item's notes are in the options below.
            if !picked {
                super::tools::placeable_info(ui, p.sim, shown);
            }
        }
        Item::RailPiece(f) => {
            figures(ui, Placeable::Rail(f));
            let (w, d) = nd_sim::Facility::size(f);
            row(ui, "footprint", format!("{w:.0} × {d:.0} m, track on one side, a street on the other"));
        }
        Item::Road(label) => {
            hint(ui, super::tools::preset_hint(label));
            if let Some((_, f, b, k)) = ROAD_PRESETS.iter().find(|x| x.0 == label) {
                if *k == RoadKind::Street {
                    row(ui, "lanes", format!("{f} forward, {b} back"));
                }
            }
            if label == "Roundabout" {
                hint(ui, help::ROAD_ROUNDABOUT);
            } else if label == "Interchange" {
                hint(ui, help::ROAD_INTERCHANGE);
            } else if label == "Path" {
                hint(ui, help::ROAD_PATH);
            }
        }
        Item::Track => hint(ui, help::ROAD_RAIL),
        Item::Beach => {
            let n = p.sim.world.shore.beaches.len();
            let open = p.sim.world.shore.beaches.values().filter(|b| b.usable()).count();
            row(ui, "beaches", format!("{n} ({open} open and reachable) · {} on the sand now", p.sim.world.shore.bathers()));
            hint(ui, BEACH_NOTE);
        }
        Item::Zone(lu) => {
            let demand = p.sim.world.economy.demand_for(lu);
            let col = zone_color32(p.assets.palette.zone_color(lu));
            ui.add(egui::ProgressBar::new(demand).text(format!("demand {:.0}%", demand * 100.0)).fill(col.gamma_multiply(0.7)).desired_height(18.0));
            hint(ui, zone_note(lu));
        }
        Item::District => hint(ui, super::districts::TOOL_HINT),
        Item::BusLines => {
            let t = &p.sim.world.economy.transit;
            row(ui, "network", format!("{} lines · {} depots for {} buses", p.sim.world.transit.routes.len(), t.depots, t.depot_capacity));
            hint(ui, help::tool_hint(Tool::Transit));
            if t.depots == 0 {
                hint(ui, "Buses need a depot first: Transit, Buses, Bus depot.");
            }
            let taxis = p.sim.world.taxis.counts();
            hint(ui, &format!("Taxis run themselves: a company opens where people want cabs ({} cabs now).", taxis.cabs));
        }
        Item::MetroLines => {
            let s = &p.sim.world.subway.stats;
            row(ui, "network", format!("{} stations · {} lines · {:.1} km", s.stations, s.lines, s.tunnel_m / 1000.0 + 0.0));
            // [metro-ux]
            hint(ui, "Click stations in order on the Metro layer (M); the tunnel to each is drawn as you go. Click the first station again for a loop; Enter or Finish lays the line.");
            if s.stations < 2 {
                hint(ui, "Place two metro stations first: Transit, Metro, Metro station.");
            }
        }
    }
}

/// Cost, upkeep, jobs, size and coverage of a placeable, as a small table.
fn figures(ui: &mut egui::Ui, what: Placeable) {
    egui::Grid::new(("build_figures", format!("{what:?}"))).num_columns(2).spacing(egui::vec2(10.0, 1.0)).show(ui, |ui| {
        let mut r = |label: &str, value: String| {
            ui.label(egui::RichText::new(label).small().weak());
            ui.label(egui::RichText::new(value).small());
            ui.end_row();
        };
        // [plant-groups] Solar and wind: per 1,000 m² of lot; the card by
        // the pointer shows the figures for the lot under it.
        let per = match what {
            Placeable::Plant(k) => k.reference_area().map(|a| 1000.0 / a),
            _ => None,
        };
        match per {
            Some(f) => {
                r("cost", format!("$ {:.0} per 1000 m²", what.cost() * f));
                r("upkeep", format!("$ {:.0} a day per 1000 m²", what.upkeep() * f));
            }
            None => {
                r("cost", format!("$ {:.0}", what.cost()));
                r("upkeep", format!("$ {:.0} a day", what.upkeep()));
            }
        }
        if what.jobs() > 0 {
            r("jobs", format!("{}", what.jobs()));
        }
        if what.min_area() > 0.0 {
            r("lot", format!("{:.0} m² or more", what.min_area()));
        }
        if let Placeable::Plant(k) = what {
            match per {
                // [plant-scale] Thermal plants: sized to their site, capped.
                Some(f) if k.sized_when_built() => {
                    let (lo, hi) = k.size_range();
                    r("output", format!("{:.0} MW per 1000 m²: {:.0} MW on {:.0} m² up to {:.0} MW on {:.0} m² (more ground is yard)", k.capacity_mw() * f, k.capacity_at(lo), k.min_area(), k.capacity_at(hi), k.max_area()));
                }
                Some(f) => {
                    r("output", format!("{:.0} MW per 1000 m², up to {:.0} MW a lot", k.capacity_mw() * f, k.capacity_mw() * nd_sim::power::MAX_LOT_SCALE));
                    r("groups", format!("touching lots join one {} (+{:.0}% a lot, up to +{:.0}%)", if k == nd_sim::PowerPlantKind::Solar { "Sunfield" } else { "Wind Park" }, nd_sim::world::plant_groups::PLANT_GROUP_BONUS_PER_LOT * 100.0, nd_sim::world::plant_groups::PLANT_GROUP_BONUS_MAX * 100.0));
                }
                None => r("output", format!("{:.0} MW (fixed size)", k.capacity_mw())),
            }
        }
        if let Placeable::Service(k, t) = what {
            if k.radius(t) > 0.0 {
                r("coverage", format!("{:.0} m", k.radius(t)));
            }
            if k.units(t) > 0 {
                r("units", format!("{} {}", k.units(t), k.unit_kind().map_or("", |u| u.label())));
            }
            if k.capacity(t) > 0 {
                r("capacity", format!("{}", k.capacity(t)));
            }
        }
        if let Placeable::Dock(k) = what {
            let (w, d) = k.size();
            r("footprint", format!("{w:.0} × {d:.0} m"));
            if k == PortKind::ShippingPort {
                r("water", format!("{:.0} m wide or the sea", k.min_width()));
            }
            // [economy]
            if k.berths() > 0 {
                r("berths", format!("{}", k.berths()));
                r("water", format!("{:.0} m wide, a lake or the sea", k.min_width()));
            }
        }
    });
}

fn row(ui: &mut egui::Ui, label: &str, value: String) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).small().weak());
        ui.label(egui::RichText::new(value).small());
    });
}

fn zone_note(lu: LandUse) -> &'static str {
    match lu {
        LandUse::Residential => "Homes. Newcomers move in when there are jobs; rents follow land value.",
        LandUse::Commercial => "Shops and offices: jobs, groceries and goods. Needs a street out front.",
        LandUse::Industrial => "Works yards and plants: jobs and goods, noise and pollution. Pick a sub-zone in the options.",
        LandUse::Entertainment => "Bars, clubs, cinemas: nightlife and jobs, some noise at night.",
        LandUse::Civic => "Public buildings: lifts land value around them.",
        LandUse::Park => "Open green: lifts land value and mood nearby; goes anywhere, rough ground too.",
        #[allow(unreachable_patterns)]
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_placeable_is_in_exactly_one_section() {
        for (_, item) in placeables() {
            let n = Cat::ALL.iter().filter(|c| groups(**c).iter().any(|g| g.items.iter().any(|i| i.same(item)))).count();
            assert_eq!(n, 1, "{item:?} listed {n} times");
        }
    }

    #[test]
    fn picking_an_item_selects_it() {
        for cat in Cat::ALL {
            for g in groups(cat) {
                for item in g.items {
                    let mut t = ToolState::default();
                    activate(item, &mut t);
                    let now = current_item(&t).expect("a build tool");
                    assert!(now.same(item), "{item:?} came back as {now:?}");
                    assert_eq!(cat_of(now), cat, "{item:?}");
                }
            }
        }
    }

    #[test]
    fn highlight_names_resolve_to_a_section_a_sub_section_or_a_kind() {
        assert_eq!(resolve_highlight("power"), Some((Cat::Power, None, None)));
        assert_eq!(resolve_highlight("safety"), Some((Cat::Services, Some("Safety"), None)));
        assert_eq!(resolve_highlight("parking"), Some((Cat::Transit, Some("Parking"), None)));
        let (cat, sub, item) = resolve_highlight("health").unwrap();
        assert_eq!((cat, sub), (Cat::Services, Some("Health and education")));
        assert_eq!(item.and_then(|i| i.placeable()).map(norm), Some(Placeable::Service(ServiceKind::Health, Tier::Mid)));
        assert_eq!(resolve_highlight("police").map(|r| r.1), Some(Some("Safety")));
        assert_eq!(resolve_highlight("metro_station").map(|r| (r.0, r.1)), Some((Cat::Transit, Some("Metro"))));
        assert!(resolve_highlight("nonsense").is_none());
        // Roads, zones and tolls put down no buildings to show.
        assert!(highlight_kinds(Cat::Roads, None).is_empty());
        assert!(highlight_kinds(Cat::Zoning, None).is_empty());
        assert!(highlight_kinds(Cat::Transit, Some("Tolls")).is_empty());
        assert!(!highlight_kinds(Cat::Rail, None).is_empty(), "stations and yards");
    }

    #[test]
    fn a_picked_item_is_the_focus_only_inside_the_section_shown() {
        let mut t = ToolState::default();
        activate(Item::Place(Placeable::Service(ServiceKind::Health, Tier::High)), &mut t);
        let sel = highlight_sel(Some(Cat::Services), Some("Health and education"), &t).unwrap();
        assert_eq!(sel.focus, Some(Placeable::Service(ServiceKind::Health, Tier::Mid)));
        assert_eq!(highlight_sel(Some(Cat::Services), Some("Safety"), &t).unwrap().focus, None);
        assert!(highlight_sel(Some(Cat::Roads), None, &t).is_none());
    }

    #[test]
    fn rail_track_is_only_under_rail() {
        assert!(!groups(Cat::Roads).iter().any(|g| g.items.contains(&Item::Track)));
        assert!(groups(Cat::Rail).iter().any(|g| g.items.contains(&Item::Track)));
    }
}

/// [waterfront] What a public beach is, for the Build window.
pub const BEACH_NOTE: &str = "Public beach: paint it on parcels that run down to the water. People walk to it from a street or path it fronts, or through a park next to it (without one it stays empty: no access). Warm, dry afternoons fill it; cold, rain and night empty it, and Arctic beaches stay deserted. Fouled water from dirty industry closes it. Open beaches lift land value and mood nearby, draw tourists and bring beach bars.";
