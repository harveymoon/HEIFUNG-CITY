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
const ROADS: [&str; 9] = ["Path", "Lane", "Street", "Avenue", "One-way", "Ramp", "Highway", "Tunnel", "Roundabout"];

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
            Item::Zone(lu) => lu.label().to_string(),
            Item::District => "Paint districts".into(),
            Item::Place(Placeable::Plant(k)) => format!("{} {:.0} MW", k.label(), k.capacity_mw()),
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
        ServiceKind::Health | ServiceKind::School => "Health and education",
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
                    "Highway" | "Ramp" => "Highways",
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
        Cat::Parks => add("Parks", Item::Zone(LandUse::Park)),
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
        Tool::Zone => Some(Item::Zone(t.land_use)),
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
        Item::Zone(_) => Tool::Zone,
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

/// The section the window shows (for `status`).
pub fn section_of(ctx: &egui::Context) -> Option<Cat> {
    ctx.data(|d| d.get_temp::<State>(state_id())).and_then(|s| s.cat)
}

/// Follow the tools: a building tool picked by key or by the API opens the
/// window at its section. Called every frame, open or not.
pub fn follow_tools(ctx: &egui::Context, tools: &ToolState) {
    let mut st: State = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();
    let now = current_item(tools);
    if st.last_tool != Some(tools.tool) && st.last_tool.is_some() && is_build_tool(tools.tool) {
        set_open(ctx, true);
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
const TILE: egui::Vec2 = egui::vec2(104.0, 128.0);
const THUMB: f32 = 96.0;
const TREE_WIDTH: f32 = 150.0;
const INFO_WIDTH: f32 = 330.0;

pub fn window(ctx: &egui::Context, p: &mut UiCtx, content: Rect) {
    follow_tools(ctx, p.tools);
    if !is_open(ctx) {
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
    // Follow a pick made in the grid this frame.
    st.last = current_item(p.tools);
    st.last_tool = Some(p.tools.tool);
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
    if !keep {
        set_open(ctx, false);
    }
}

fn tree(ui: &mut egui::Ui, st: &mut State) {
    for cat in Cat::ALL {
        let on = st.cat == Some(cat);
        let text = egui::RichText::new(cat.label()).strong();
        if ui.add_sized(egui::vec2(TREE_WIDTH - 14.0, 22.0), egui::Button::selectable(on && st.sub.is_none(), if on { text.color(GOLD) } else { text })).clicked() {
            st.cat = Some(cat);
            st.sub = None;
        }
        if on {
            let groups = groups(cat);
            if groups.len() > 1 {
                for g in &groups {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        if ui.selectable_label(st.sub == Some(g.name), egui::RichText::new(g.name).small()).clicked() {
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
    let price = item.placeable().map(|pl| format!("{:.0}", pl.cost()));
    let font = egui::FontId::proportional(11.0);
    painter.text(egui::pos2(rect.center().x, img.bottom() + 3.0), egui::Align2::CENTER_TOP, elide(&name, 17), font.clone(), if on { Color32::WHITE } else { IVORY });
    if let Some(price) = price {
        painter.text(egui::pos2(rect.center().x, img.bottom() + 15.0), egui::Align2::CENTER_TOP, format!("$ {price}"), egui::FontId::proportional(10.0), Color32::from_rgb(170, 160, 140));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn elide(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

/// An icon for items with no building to show (and while thumbnails render).
fn icon(painter: &egui::Painter, r: Rect, item: Item, p: &UiCtx, col: Color32) {
    match item {
        Item::Road("Roundabout") => icons::roundabout(painter, r, col),
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
            painter.text(r.center(), egui::Align2::CENTER_CENTER, "…", egui::FontId::proportional(18.0), col);
        }
    }
}

/// What an item costs and does: the hovered one, else the one picked.
fn info(ui: &mut egui::Ui, p: &mut UiCtx, item: Item, hovered: bool) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(item.label()).color(GOLD).strong().size(15.0));
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
            } else if label == "Path" {
                hint(ui, help::ROAD_PATH);
            }
        }
        Item::Track => hint(ui, help::ROAD_RAIL),
        Item::Zone(lu) => {
            let demand = p.sim.world.economy.demand_for(lu);
            let col = zone_color32(p.assets.palette.zone_color(lu));
            ui.add(egui::ProgressBar::new(demand).text(format!("demand {:.0}%", demand * 100.0)).fill(col.gamma_multiply(0.7)).desired_height(12.0));
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
            row(ui, "network", format!("{} stations · {} lines · {:.1} km", s.stations, s.lines, s.tunnel_m / 1000.0));
            hint(ui, "Click lots for stations (or pick standing ones and the Central Station) in order; Enter or right-click lays the line through tunnels. U shows the underground.");
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
        r("cost", format!("$ {:.0}", what.cost()));
        r("upkeep", format!("$ {:.0} a day", what.upkeep()));
        if what.jobs() > 0 {
            r("jobs", format!("{}", what.jobs()));
        }
        if what.min_area() > 0.0 {
            r("lot", format!("{:.0} m² or more", what.min_area()));
        }
        if let Placeable::Plant(k) = what {
            r("output", format!("{:.0} MW", k.capacity_mw()));
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
    fn rail_track_is_only_under_rail() {
        assert!(!groups(Cat::Roads).iter().any(|g| g.items.contains(&Item::Track)));
        assert!(groups(Cat::Rail).iter().any(|g| g.items.contains(&Item::Track)));
    }
}
