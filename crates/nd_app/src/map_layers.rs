//! The map layers: one data view over the city at a time, picked from the
//! top bar's Layer button (or the View window, a hotkey, the remote API's
//! `overlay`), always with its colour key in the legend.
//!
//! # Adding a layer
//!
//! A layer is one [`MapLayer`] entry in [`MAP_LAYERS`]: its API key, label,
//! group in the picker, hotkey text, hover hint, the `Overlays` flag the
//! renderer reads, and the function that draws its key. The renderer keeps
//! reading its own `bool` on [`Overlays`] (add the field and its default);
//! everything else (the picker, the View window, the legend, the API's
//! `overlay` command and `status.overlays`, exclusivity) follows the entry.
//!
//! The land survey layers (`nd_sim::survey::LandLayer`) are listed after
//! the entries, generated from `LandLayer::ALL`; they share
//! `Overlays::land`.
//!
//! # One at a time
//!
//! Layers are exclusive. [`set_active`] turns one on and every other off;
//! [`keep_exclusive`] runs every frame and, when code anywhere switched a
//! second flag on directly (a hotkey toggling `overlays.land_value`, the
//! District tool turning its layer on), keeps the newest and turns the
//! rest off. So older code that flips a flag stays correct.

use bevy::prelude::*;
use bevy_egui::egui::{self, Color32};
use nd_procgen::overlay::{crime_color, heat_color, pollution_color, traffic_color, VehicleClass};
use nd_sim::survey::LandLayer;
use nd_sim::LandUse;

use crate::assets::GameAssets;
use crate::render::Overlays;

/// What a legend function may read.
pub struct LegendCtx<'a> {
    pub assets: &'a GameAssets,
    pub world: &'a nd_sim::World,
}

/// One map layer the player can show.
pub struct MapLayer {
    /// API name (`overlay {layer}`), also the id kept in egui memory.
    pub key: &'static str,
    pub label: &'static str,
    /// Heading it is listed under in the picker.
    pub group: &'static str,
    /// The key that toggles it, for the label ("" for none).
    pub hotkey: &'static str,
    /// What it shows, for the picker's hover text.
    pub hint: &'static str,
    /// The flag on [`Overlays`] the renderer reads.
    pub flag: fn(&mut Overlays) -> &mut bool,
    /// Draw its colour key into the legend.
    pub legend: fn(&mut egui::Ui, &LegendCtx),
}

/// Every flag layer, in picker order. Add new layers here (see the module
/// docs).
pub const MAP_LAYERS: &[MapLayer] = &[
    MapLayer { key: "zoning", label: "Zoning", group: "City", hotkey: "O", hint: "Zone outline glows and vacant lots (the ground tint stays)", flag: |o| &mut o.zoning, legend: legend_zoning },
    MapLayer { key: "land_value", label: "Land value", group: "City", hotkey: "V", hint: "What the land is worth: rents and tiers follow it", flag: |o| &mut o.land_value, legend: legend_land_value },
    // [r4-tiles] (render/land_tiles.rs, ui/land.rs)
    MapLayer { key: "land_tiles", label: "Land for sale", group: "City", hotkey: "", hint: "The land round the city in tiles: what each holds and what it costs; buy the next one", flag: |o| &mut o.land_tiles, legend: legend_land_tiles },
    MapLayer { key: "districts", label: "Districts", group: "City", hotkey: "", hint: "Named districts: areas, borders and names", flag: |o| &mut o.districts, legend: legend_districts },
    MapLayer { key: "traffic", label: "Traffic load", group: "Traffic", hotkey: "L", hint: "Per lane: how full and how slow", flag: |o| &mut o.traffic, legend: legend_traffic },
    MapLayer { key: "traffic_types", label: "Traffic types", group: "Traffic", hotkey: "L", hint: "A disc under every vehicle by type", flag: |o| &mut o.traffic_types, legend: legend_traffic_types },
    // [clarity] (render/layers.rs `danger_gizmos`)
    MapLayer { key: "danger", label: "Traffic danger", group: "Traffic", hotkey: "", hint: "Crashes on the record per street: where accidents keep happening", flag: |o| &mut o.danger, legend: legend_danger },
    MapLayer { key: "paths", label: "Pedestrian network", group: "Traffic", hotkey: "P", hint: "Lot loops, alleys and gates people walk", flag: |o| &mut o.paths, legend: legend_paths },
    // [walk-networks] (render/overlays.rs `sync_walk_networks`)
    MapLayer { key: "walk_networks", label: "Walk networks", group: "Traffic", hotkey: "", hint: "Every walkway coloured by the network it belongs to: places people cannot walk between show in different colours; buildings off the main network outlined", flag: |o| &mut o.walk_networks, legend: legend_walk_networks },
    MapLayer { key: "people", label: "People", group: "People", hotkey: "L", hint: "Where everyone is right now", flag: |o| &mut o.people, legend: legend_people },
    MapLayer { key: "pollution", label: "Pollution and noise", group: "Environment", hotkey: "L", hint: "From plants, industry, nightlife and traffic", flag: |o| &mut o.pollution, legend: legend_pollution },
    // [layers] (render/focus.rs, render/layers.rs, render/mapview.rs)
    MapLayer { key: "crime", label: "Crime", group: "People", hotkey: "L", hint: "Where offences pile up; police cars and their routes lit over a dimmed city, the police stations marked and the ground they cover outlined", flag: |o| &mut o.crime, legend: legend_crime },
    // [events] (render/nights.rs)
    MapLayer { key: "road_condition", label: "Road condition", group: "Traffic", hotkey: "", hint: "Wear on every street: green is smooth, red is crumbling; crews fix dear land first", flag: |o| &mut o.road_condition, legend: legend_road_condition },
    MapLayer { key: "map", label: "Map view", group: "City", hotkey: "L", hint: "A street map: roads, street names, zones and pins, buildings and people hidden", flag: |o| &mut o.map, legend: legend_map },
    // [metro-ux] (metro_ux.rs, render/focus.rs) The metro line tool shows it.
    // [pt-ux] (render/pt_layers.rs)
    MapLayer { key: "parking", label: "Parking", group: "Traffic", hotkey: "", hint: "Streets by the legal parking a driver finds (kerbs and public lots in walking reach); every public lot with how full it is", flag: |o| &mut o.parking, legend: legend_parking },
    MapLayer { key: "trails", label: "Trails", group: "People", hotkey: "", hint: "A long exposure: everything that moves leaves a fading streak over a darkened city", flag: |o| &mut o.trails, legend: legend_trails },
    MapLayer { key: "density", label: "Land density", group: "City", hotkey: "", hint: "Foot traffic and people per block, smoothed over days: quiet suburbs dark, busy main streets hot, where developers build taller", flag: |o| &mut o.density, legend: legend_density },
    // [park-access] (render/pt_layers.rs `park_access_gizmos`)
    MapLayer { key: "park_access", label: "Park access", group: "People", hotkey: "", hint: "Walking minutes from every building to the nearest open park: green close, red far, homes with no park in reach ringed", flag: |o| &mut o.park_access, legend: legend_park_access },
    // [w1] (render/sites_layer.rs)
    MapLayer { key: "construction", label: "Construction", group: "City", hotkey: "", hint: "Building sites: going up (with the planned height), coming down for a rebuild, or coming down for good", flag: |o| &mut o.construction, legend: legend_construction },
    MapLayer { key: "metro", label: "Metro", group: "Transit", hotkey: "M", hint: "The metro: tunnels and lines in their colours, every station a named marker, over a dimmed city", flag: |o| &mut o.metro, legend: legend_metro },
];

/// A layer: a flag entry of [`MAP_LAYERS`] or a land survey layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerId {
    Flag(usize),
    Land(LandLayer),
}

impl LayerId {
    /// Every layer in picker order: the entries, then the land layers.
    pub fn all() -> Vec<LayerId> {
        (0..MAP_LAYERS.len()).map(LayerId::Flag).chain(LandLayer::ALL.into_iter().map(LayerId::Land)).collect()
    }

    pub fn key(self) -> &'static str {
        match self {
            LayerId::Flag(i) => MAP_LAYERS[i].key,
            LayerId::Land(l) => l.key(),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LayerId::Flag(i) => MAP_LAYERS[i].label,
            LayerId::Land(l) => l.label(),
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            LayerId::Flag(i) => MAP_LAYERS[i].group,
            LayerId::Land(l) if l.is_industry() => "Industry odds",
            LayerId::Land(_) => "Land and resources",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            LayerId::Flag(i) => MAP_LAYERS[i].hint,
            LayerId::Land(l) => l.hint(),
        }
    }

    pub fn hotkey(self) -> &'static str {
        match self {
            LayerId::Flag(i) => MAP_LAYERS[i].hotkey,
            LayerId::Land(_) => "K",
        }
    }

    /// From an API key (case-insensitive).
    pub fn parse(key: &str) -> Option<LayerId> {
        let k = key.trim().to_ascii_lowercase();
        MAP_LAYERS.iter().position(|l| l.key == k).map(LayerId::Flag).or_else(|| LandLayer::from_key(&k).map(LayerId::Land))
    }

    fn is_on(self, o: &mut Overlays) -> bool {
        match self {
            LayerId::Flag(i) => *(MAP_LAYERS[i].flag)(o),
            LayerId::Land(l) => o.land == Some(l),
        }
    }
}

/// Every layer key, for error messages and the API's help.
pub fn keys() -> Vec<&'static str> {
    LayerId::all().into_iter().map(LayerId::key).collect()
}

/// The layer showing, if any (the first on, should two be on mid-frame).
pub fn active(o: &Overlays) -> Option<LayerId> {
    // `flag` wants `&mut`; reading through a copy of the flags is enough.
    on_layers(&mut o.clone()).into_iter().next()
}

fn on_layers(o: &mut Overlays) -> Vec<LayerId> {
    let mut on: Vec<LayerId> = (0..MAP_LAYERS.len()).map(LayerId::Flag).filter(|l| l.is_on(o)).collect();
    if let Some(l) = o.land {
        on.push(LayerId::Land(l));
    }
    on
}

/// Show `layer` alone, or none.
pub fn set_active(o: &mut Overlays, layer: Option<LayerId>) {
    for l in MAP_LAYERS {
        *(l.flag)(o) = false;
    }
    o.land = None;
    match layer {
        Some(LayerId::Flag(i)) => *(MAP_LAYERS[i].flag)(o) = true,
        Some(LayerId::Land(l)) => o.land = Some(l),
        None => {}
    }
}

/// [fix/review-app] The layer hotkeys: O zoning, V land value, P paths,
/// L the people/traffic/pollution/crime cycle, K the land layers. Read
/// here, before `keep_exclusive`, not inside the render systems in Sync:
/// there two exclusive layers showed together for a frame, and the systems
/// earlier in the chain saw the toggle a frame late.
fn layer_hotkeys(keys: Res<ButtonInput<KeyCode>>, flags: Res<crate::ui::UiFlags>, mode: Option<Res<crate::camera::CameraMode>>, mut overlays: ResMut<Overlays>) {
    // [r5-drive] At the wheel V, L and P are the car's.
    if flags.keyboard_captured || mode.is_some_and(|m| *m == crate::camera::CameraMode::Drive) {
        return;
    }
    if keys.just_pressed(KeyCode::KeyO) {
        overlays.zoning = !overlays.zoning;
    }
    if keys.just_pressed(KeyCode::KeyV) {
        overlays.land_value = !overlays.land_value;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        overlays.paths = !overlays.paths;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        overlays.cycle();
    }
    if keys.just_pressed(KeyCode::KeyK) {
        overlays.cycle_land();
    }
}

/// Keep one layer on: when a second came on since the last frame, the
/// newest stays. Runs in `PreUpdate`, so whatever turned a flag on last
/// frame is settled before the renderer reads the flags.
pub fn keep_exclusive(mut overlays: ResMut<Overlays>, mut last: Local<Option<LayerId>>) {
    let on = on_layers(overlays.bypass_change_detection());
    if on.len() > 1 {
        let keep = on.iter().copied().find(|l| Some(*l) != *last).unwrap_or(on[0]);
        set_active(&mut overlays, Some(keep));
        *last = Some(keep);
    } else {
        *last = on.first().copied();
    }
}

pub struct MapLayersPlugin;

impl Plugin for MapLayersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, (layer_hotkeys, keep_exclusive).chain().after(bevy::input::InputSystems));
    }
}

// ── legends ─────────────────────────────────────────────────────────────────

/// Draw the colour key of `layer`.
pub fn legend(ui: &mut egui::Ui, layer: LayerId, cx: &LegendCtx) {
    match layer {
        LayerId::Flag(i) => (MAP_LAYERS[i].legend)(ui, cx),
        LayerId::Land(l) => crate::ui::legend::land_legend(ui, l),
    }
}

fn linear(c: [f32; 3]) -> Color32 {
    Color32::from(egui::Rgba::from_rgb(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)))
}

fn small(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().weak());
}

/// A horizontal colour ramp with its end labels.
pub fn ramp(ui: &mut egui::Ui, color: impl Fn(f32) -> Color32, lo: &str, hi: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(180.0, 10.0), egui::Sense::hover());
    let steps = 36;
    for i in 0..steps {
        let t = (i as f32 + 0.5) / steps as f32;
        let x0 = rect.left() + rect.width() * i as f32 / steps as f32;
        let x1 = rect.left() + rect.width() * (i + 1) as f32 / steps as f32 + 0.5;
        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom())), 0.0, color(t));
    }
    ui.horizontal(|ui| {
        ui.set_width(180.0);
        small(ui, lo);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| small(ui, hi));
    });
}

/// A swatch and a name per entry, two to a row.
pub fn swatches(ui: &mut egui::Ui, items: &[(Color32, String)], round: bool) {
    egui::Grid::new(("legend_swatches", items.len())).num_columns(2).spacing(egui::vec2(10.0, 2.0)).show(ui, |ui| {
        for (i, (col, name)) in items.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                if round {
                    ui.painter().circle_filled(r.center(), 5.0, *col);
                } else {
                    ui.painter().rect_filled(r, 2.0, *col);
                }
                ui.label(egui::RichText::new(name).small());
            });
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
}

fn legend_zoning(ui: &mut egui::Ui, cx: &LegendCtx) {
    // [r4-leisure] leisure is commercial land's sub-zone
    let items: Vec<(Color32, String)> = LandUse::ALL.iter().map(|lu| (linear(cx.assets.palette.zone_color(*lu)), if *lu == LandUse::Entertainment { "Commercial · leisure".to_string() } else { lu.label().to_string() })).collect();
    swatches(ui, &items, false);
    small(ui, "glowing outlines: zoned lots by use; plain outlines: vacant lots; pale steel outlines: docks and stations (their own ground, no zone)");
}

fn legend_land_value(ui: &mut egui::Ui, cx: &LegendCtx) {
    let (a, b) = (cx.assets.palette.overlay_low, cx.assets.palette.overlay_high);
    ramp(ui, |t| linear([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]), "low", "high");
    small(ui, "rents and building tiers follow it");
}

fn legend_districts(ui: &mut egui::Ui, cx: &LegendCtx) {
    let d = &cx.world.districts;
    if d.list.is_empty() {
        small(ui, "no districts yet: the Build window's Districts section paints them");
        return;
    }
    let items: Vec<(Color32, String)> = d.list.iter().map(|x| (linear(nd_sim::districts::COLORS[x.color as usize % nd_sim::districts::COLORS.len()]), x.name.clone())).collect();
    swatches(ui, &items, false);
}

fn legend_traffic(ui: &mut egui::Ui, _: &LegendCtx) {
    ramp(ui, |t| linear(traffic_color(t)), "free flow", "jammed");
    small(ui, "per lane: how full and how slow");
    swatches(ui, &[(linear(crate::render::TRAFFIC_BLUE), "every vehicle".into()), (linear([0.25, 0.5, 1.0]), "tunnels, seen through".into()), (linear([3.5, 0.3, 0.15]), "a crash in the road".into())], false);
    small(ui, "the city is dimmed; load lines, crashes and cars in tunnels show through everything");
}

fn legend_land_tiles(ui: &mut egui::Ui, ctx: &LegendCtx) {
    let items: Vec<(Color32, String)> = [(crate::render::land_tiles::OWNED, "the city's land"), (crate::render::land_tiles::FOR_SALE, "for sale now"), (crate::render::land_tiles::LATER, "for sale later")].iter().map(|(rgb, t)| (linear(*rgb), t.to_string())).collect();
    swatches(ui, &items, false);
    small(ui, "click a price on the map, or pick a tile in the Land for sale window");
    let w = ctx.world;
    let n = w.landscape.tiles.len();
    small(ui, &format!("tiles of {:.1} km; {n} of {} bought; each costs more than the last", nd_sim::landscape::TILE / 1000.0, nd_sim::landscape::outer_tiles().len()));
}

fn legend_danger(ui: &mut egui::Ui, ctx: &LegendCtx) {
    let top = 2 * nd_sim::world::journeys::DANGEROUS_STREET;
    ramp(ui, |t| linear(crate::render::danger_color(1 + (t * (top - 1) as f32).round() as u32)), "1 crash", &format!("{top}+ crashes"));
    small(ui, &format!("per street, drawn over everything; rings are the crashes. From {} on a street, people living on it complain of a dangerous street", nd_sim::world::journeys::DANGEROUS_STREET));
    let worst = ctx.world.crashes_per_segment().first().map_or(0, |x| x.1);
    small(ui, &format!("{} crashes on the record; the worst street has {worst}", ctx.world.journeys.crash_sites.len()));
}

fn legend_crime(ui: &mut egui::Ui, _: &LegendCtx) {
    ramp(ui, |t| {
        let c = crime_color(t);
        linear([c[0], c[1], c[2]])
    }, "quiet", "hot spot");
    small(ui, "offences lately, fading a quarter a day");
    swatches(
        ui,
        &[
            (linear(crate::render::POLICE_GLOW), "police car".into()),
            (linear([0.3, 0.9, 1.3]), "patrol route".into()),
            (linear([2.2, 0.25, 0.3]), "on a call".into()),
            (linear([1.6, 0.4, 0.1]), "open crime".into()),
            (linear(crate::build_highlight::Family::Police.linear()), "police station and its cover".into()),
        ],
        false,
    );
}

fn legend_road_condition(ui: &mut egui::Ui, cx: &LegendCtx) {
    ramp(ui, |t| linear(crate::render::road_wear_color(t)), "new", "crumbling");
    let w = cx.world;
    let by = w.road_wear_by_land_value();
    small(ui, &format!("worn past 35%: slower and riskier · mean wear cheap {:.0}% / middle {:.0}% / dear {:.0}%", by[0].0 * 100.0, by[1].0 * 100.0, by[2].0 * 100.0));
}

fn legend_map(ui: &mut egui::Ui, _: &LegendCtx) {
    let roads: Vec<(Color32, String)> = crate::render::MAP_ROAD_KEY.iter().map(|(l, c)| (linear(*c), l.to_string())).collect();
    swatches(ui, &roads, false);
    // The tints are dark on the map; shown brighter here so they read.
    let zones: Vec<(Color32, String)> = LandUse::ALL.iter().map(|lu| {
        let c = crate::render::map_zone_tint(*lu);
        (linear([c[0] * 4.0, c[1] * 4.0, c[2] * 4.0]), lu.label().to_string())
    }).collect();
    swatches(ui, &zones, false);
    let pins: Vec<(Color32, String)> = crate::render::MAP_PIN_KEY.iter().map(|(g, c, l)| (Color32::from_rgb(c[0], c[1], c[2]), format!("{g} {l}"))).collect();
    swatches(ui, &pins, true);
    small(ui, "street names along the roads; tunnels dashed");
}

/// [metro-ux]
fn legend_metro(ui: &mut egui::Ui, cx: &LegendCtx) {
    let w = cx.world;
    let lines: Vec<(Color32, String)> = w.subway.lines.values().map(|l| (linear(nd_procgen::metro::line_colour(l.color)), format!("{}{}", l.name, if l.looped { " (loop)" } else { "" }))).collect();
    if lines.is_empty() {
        small(ui, "no metro line yet");
    } else {
        swatches(ui, &lines, false);
    }
    swatches(ui, &[(Color32::from_rgb(120, 235, 255), format!("{} stations", w.metro_stations().len())), (Color32::WHITE, "line being drawn".into())], true);
    small(ui, "Build window, Transit, Metro: Metro station places them, Metro lines joins them");
}

/// [w1] The Construction layer's key.
fn legend_construction(ui: &mut egui::Ui, cx: &LegendCtx) {
    use nd_sim::world::sites::SiteKind;
    let sites = cx.world.construction_sites();
    let items: Vec<(Color32, String)> = SiteKind::ALL
        .iter()
        .map(|k| {
            let c = crate::render::sites_layer::site_color(*k);
            let m = c[0].max(c[1]).max(c[2]).max(1.0);
            (linear([c[0] / m, c[1] / m, c[2] / m]), format!("{} ({})", k.label(), sites.iter().filter(|s| s.1 == *k).count()))
        })
        .collect();
    swatches(ui, &items, false);
    small(ui, "boxes stand to the planned height; the bright part of a site going up is built so far");
}

fn legend_traffic_types(ui: &mut egui::Ui, _: &LegendCtx) {
    let items: Vec<(Color32, String)> = VehicleClass::ALL.iter().map(|c| (linear(c.color()), c.label().to_string())).collect();
    swatches(ui, &items, true);
}

fn legend_paths(ui: &mut egui::Ui, cx: &LegendCtx) {
    let p = cx.assets.palette.preview;
    // [pt-ux] Every walkway, by kind (`render::overlays::path_style`), and
    // the people glowing.
    swatches(
        ui,
        &[
            (linear([p[0] * 0.9, p[1] * 0.9, p[2] * 0.9]), "lot loops, alleys, gates".into()),
            (linear([0.12, 0.45, 0.55]), "pavements".into()),
            (linear([0.9, 0.9, 0.95]), "crossings".into()),
            (linear([0.25, 0.9, 0.35]), "park paths".into()),
            (linear([0.55, 0.3, 1.1]), "footbridges, raised walks".into()),
            (linear([0.5, 2.4, 3.2]), "people on foot".into()),
        ],
        false,
    );
    small(ui, &format!("{} walking now", cx.world.citizens.values().filter(|c| c.is_visible() && !matches!(c.activity, nd_sim::Activity::Driving { .. })).count()));
}

/// [walk-networks] The Walk networks layer's key: the networks by size,
/// each with its colour; hover for where it is and what is on it, click to
/// fly there.
fn legend_walk_networks(ui: &mut egui::Ui, cx: &LegendCtx) {
    let nets = &cx.world.walk_networks;
    if nets.list.is_empty() {
        small(ui, "no walkways yet");
        return;
    }
    let others = nets.list.len() - 1;
    small(ui, &if others == 0 { "one walk network: everywhere on it is in walking reach".to_string() } else { format!("{} walk networks: places in different colours are not joined on foot", nets.list.len()) });
    const ROWS: usize = 12;
    for (k, n) in nets.list.iter().enumerate().take(ROWS) {
        let c = n.color();
        // [ferry-islands] An island the ferries join to the rest.
        let island = cx.world.connectivity.islands.iter().any(|i| i.comp == n.comp);
        let label = format!("{}{}: {} walkways, {} building{}, {} resident{}", if k == 0 { "main, " } else if island { "island by ferry, " } else { "" }, n.name, n.edges, n.buildings.len(), crate::ui::fmt::s(n.buildings.len() as u32), n.residents, crate::ui::fmt::s(n.residents));
        let r = ui
            .horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, linear(c));
                ui.add(egui::Label::new(egui::RichText::new(label).small()).sense(egui::Sense::click()))
            })
            .inner;
        let names: Vec<String> = n.buildings.iter().take(6).filter_map(|b| cx.world.buildings.get(*b).map(|x| x.name.clone())).collect();
        let more = n.buildings.len().saturating_sub(names.len());
        let mut tip = format!("Network #{} {}\nfrom ({:.0}, {:.0}) to ({:.0}, {:.0})\n{} nodes, {} walkways ({} underground), {} workers", n.id, n.name, n.lo.x, n.lo.y, n.hi.x, n.hi.y, n.nodes, n.edges, n.underground, n.workers);
        if !names.is_empty() {
            tip.push_str(&format!("\n{}{}", names.join(", "), if more > 0 { format!(" and {more} more") } else { String::new() }));
        }
        tip.push_str("\nclick: fly there");
        if r.on_hover_text(tip).clicked() {
            request_fly(ui.ctx(), (n.lo + n.hi) * 0.5);
        }
    }
    if nets.list.len() > ROWS {
        small(ui, &format!("and {} smaller", nets.list.len() - ROWS));
    }
    small(ui, "outlined: buildings off the main network; dim: tunnels");
}

/// [walk-networks] Ask for a layer to be shown from somewhere without the
/// overlays at hand (an inspector line, an alert): `take_requests` in the
/// UI pass switches it on.
pub fn request_layer(ctx: &egui::Context, key: &'static str) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("nd_request_layer"), key));
}

/// [walk-networks] Ask the camera to fly to `at` (a legend row).
pub fn request_fly(ctx: &egui::Context, at: nd_sim::Vec2) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("nd_request_fly"), (at.x, at.y)));
}

/// The layer and the place asked for since the last frame, if any.
pub fn take_requests(ctx: &egui::Context, o: &mut Overlays) -> Option<nd_sim::Vec2> {
    if let Some(key) = ctx.data_mut(|d| d.remove_temp::<&'static str>(egui::Id::new("nd_request_layer"))) {
        if let Some(l) = LayerId::parse(key) {
            set_active(o, Some(l));
        }
    }
    ctx.data_mut(|d| d.remove_temp::<(f32, f32)>(egui::Id::new("nd_request_fly"))).map(|(x, y)| nd_sim::Vec2::new(x, y))
}

/// [walk-networks] A "show the walk networks" button after a line about
/// being cut off on foot.
pub fn walk_networks_button(ui: &mut egui::Ui) {
    if ui.small_button("Walk networks").on_hover_text("Show the Walk networks map layer: which places are joined on foot").clicked() {
        request_layer(ui.ctx(), "walk_networks");
    }
}

/// [pt-ux] The Parking layer's key.
fn legend_parking(ui: &mut egui::Ui, cx: &LegendCtx) {
    ramp(ui, |t| linear(crate::render::parking_color(t)), "no parking", "plenty");
    small(ui, "free kerb slots plus public lots within a short walk");
    let w = cx.world;
    let (mut used, mut cap, mut n) = (0u32, 0u32, 0u32);
    for (id, b) in w.buildings.iter() {
        if b.construction.is_none() && b.parking_capacity > 0 && (b.is_parking() || w.vehicles.city_lots.contains_key(&id)) {
            used += b.parking_used as u32;
            cap += b.parking_capacity as u32;
            n += 1;
        }
    }
    let (kt, ku) = w.kerb_occupancy();
    small(ui, &format!("{n} public lots: {used}/{cap} spaces used"));
    small(ui, &format!("kerbs: {ku}/{kt} slots taken"));
    small(ui, "lot outlines: green empty, red full");
}

/// [pt-ux] The Trails layer's key.
fn legend_trails(ui: &mut egui::Ui, _: &LegendCtx) {
    use crate::render::TrailMover as M;
    let items: Vec<(Color32, String)> = [(M::Car, "cars"), (M::Bus, "buses"), (M::Lorry, "lorries and vans"), (M::Siren, "sirens"), (M::Walker, "people on foot"), (M::Bot, "delivery robots")].into_iter()
        .map(|(m, l)| {
            // Over 1 to bloom on the map: scaled to its brightest channel here.
            let c = m.color();
            let k = c[0].max(c[1]).max(c[2]).max(1.0);
            (linear([c[0] / k, c[1] / k, c[2] / k]), l.to_string())
        })
        .collect();
    swatches(ui, &items, false);
    small(ui, "each streak fades out over about ten seconds");
}

/// [park-access] The Park access layer's key: the minutes ramp and how
/// the city's residents fare (`pt_layers::park_access_summary`, refreshed
/// with the layer).
fn legend_park_access(ui: &mut egui::Ui, _: &LegendCtx) {
    ramp(ui, |t| linear(crate::render::park_access_color(Some(t * crate::render::PARK_ACCESS_FAR))), "next door", "15+ min");
    swatches(ui, &[(linear(crate::render::park_access_color(None)), "no park in walking reach".to_string())], false);
    match crate::render::park_access_summary() {
        Some(s) if s.residents > 0 => {
            let pct = |n: u32| (s.share(n) * 100.0).round();
            small(ui, &format!("{} residents: {}% within 5 min, {}% within 10, {}% within 15; {}% ({} homes) with no park in reach", s.residents, pct(s.within_5), pct(s.within_10), pct(s.within_15), pct(s.none), s.homes_without));
        }
        Some(_) => small(ui, "nobody lives here yet"),
        None => small(ui, "measuring..."),
    }
    small(ui, "a park counts once it is laid out and open; minutes are by the walking paths, updated every two game hours");
}

/// [pt-ux] The Land density layer's key.
fn legend_density(ui: &mut egui::Ui, _: &LegendCtx) {
    ramp(ui, |t| {
        let c = heat_color(t);
        linear([c[0], c[1], c[2]])
    }, "low", "dense");
    small(ui, "people out on foot and in the buildings, smoothed over days: where it is hot developers buy up small plots and build taller");
}

fn legend_people(ui: &mut egui::Ui, _: &LegendCtx) {
    ramp(ui, |t| {
        let c = heat_color(t);
        linear([c[0], c[1], c[2]])
    }, "few", "crowded");
    small(ui, "everyone where they are, indoors at their building");
}

fn legend_pollution(ui: &mut egui::Ui, _: &LegendCtx) {
    ramp(ui, |t| {
        let c = pollution_color(t);
        linear([c[0], c[1], c[2]])
    }, "clean", "as by a coal plant");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_parse_back() {
        let all = LayerId::all();
        let mut seen = std::collections::HashSet::new();
        for l in &all {
            assert!(seen.insert(l.key()), "duplicate key {}", l.key());
            assert_eq!(LayerId::parse(l.key()), Some(*l));
        }
    }

    #[test]
    fn layers_are_exclusive() {
        let mut o = Overlays::default();
        set_active(&mut o, LayerId::parse("traffic"));
        assert_eq!(active(&o), LayerId::parse("traffic"));
        assert!(!o.zoning && o.traffic);
        set_active(&mut o, LayerId::parse("oil"));
        assert!(!o.traffic);
        assert_eq!(active(&o).map(LayerId::key), Some("oil"));
        set_active(&mut o, None);
        assert_eq!(active(&o), None);
    }
}
