//! The toolbar's sub-pane: one block of parameters per tool, shown for the
//! active tool only. The buttons that pick the tool live in `toolbar`.

use bevy_egui::egui;
use nd_sim::roads::RoadKind;
use nd_sim::{BuildCategory, Gesture, LandUse, Placeable, RailFacility, ServiceKind, Tier};

use super::help;
use super::{hint, icons, section, zone_color32, WARN};
use crate::assets::GameAssets;
use crate::sim::Sim;
use crate::tools::{EffectivePlan, Tool, ToolState, TransitMode, ZoneMode, BRUSH_MAX, BRUSH_MIN, ROAD_PRESETS};

pub fn sub_pane(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState, plan: &mut EffectivePlan, assets: &GameAssets) {
    match tools.tool {
        Tool::Road => road(ui, tools),
        Tool::Zone => zone(ui, sim, tools, assets),
        Tool::Edit => edit(ui, sim, tools, plan),
        Tool::Bulldoze => hint(ui, help::tool_hint(Tool::Bulldoze)),
        Tool::Inspect => hint(ui, help::tool_hint(Tool::Inspect)),
        Tool::Transit => transit(ui, sim, tools),
        Tool::Rail => rail(ui, sim, tools),
        Tool::Buildings => buildings(ui, sim, tools),
        Tool::District => super::districts::sub_pane(ui, sim, tools),
    }
}

fn road(ui: &mut egui::Ui, tools: &mut ToolState) {
    // The preset (Path, Street, Avenue, Tunnel, Roundabout, ...) is picked in
    // the Build window's Roads or Rail section; these are its parameters.
    ui.label(egui::RichText::new(format!("Drawing: {}", road_label(tools))).strong());
    if tools.roundabout {
        ui.horizontal(|ui| {
            let mut d = tools.ring_radius * 2.0;
            if ui.add(egui::Slider::new(&mut d, nd_sim::roads::RING_MIN_RADIUS * 2.0..=nd_sim::roads::RING_MAX_RADIUS * 2.0).step_by(2.0).suffix(" m").text("across [Ctrl+wheel]")).changed() {
                tools.ring_radius = d * 0.5;
            }
            ui.separator();
            for n in 1..=nd_sim::roads::RING_MAX_LANES {
                if ui.selectable_label(tools.ring_lanes == n, format!("{n} lane{}", if n == 1 { "" } else { "s" })).clicked() {
                    tools.ring_lanes = n;
                }
            }
        });
        hint(ui, help::ROAD_ROUNDABOUT);
        return;
    }
    // Lanes and curvature share a row so the bar stays shallow.
    ui.horizontal(|ui| {
        if tools.kind == RoadKind::Street {
            ui.add(egui::Slider::new(&mut tools.lanes_fwd, 0..=4).text("lanes fwd"));
            ui.add(egui::Slider::new(&mut tools.lanes_bwd, 0..=4).text("lanes back"));
            if tools.lanes_fwd + tools.lanes_bwd == 0 {
                tools.lanes_fwd = 1;
            }
            ui.separator();
        }
        if ui.selectable_label(!tools.curved, "Straight").clicked() {
            tools.curved = false;
        }
        if ui.selectable_label(tools.curved, "Curved [C]").clicked() {
            tools.curved = true;
        }
    });
    // The level the next point goes down at.
    ui.horizontal(|ui| {
        if ui.small_button("lower").on_hover_text("Lower: [ or Page Down").clicked() {
            tools.step_elevation(false);
        }
        let text = egui::RichText::new(format!("Level: {}", crate::tools::elevation_label(tools.elevation))).strong();
        ui.label(text).on_hover_text("] raises and [ lowers the next point 2 m (or Page Up / Page Down); Shift+] / Shift+[ move the whole road drawn so far. A raised road stands on concrete pillars that dodge the roads and buildings under it: red where there is no room for one (implementing is refused), buildings a pillar would buy out outlined in orange. Roads 4.5 m or more above or below another pass over or under it without a junction; a raised road stands on a viaduct, one 5 m or more down runs in a tunnel.");
        if ui.small_button("raise").on_hover_text("Raise: ] or Page Up").clicked() {
            tools.step_elevation(true);
        }
        if tools.elevation != 0.0 && !tools.tunnel && ui.small_button("ground").clicked() {
            tools.elevation = 0.0;
        }
    });
    // [tools] The raised road's columns.
    if tools.pillars.count > 0 || tools.pillars.refusal.is_some() {
        let n = &tools.pillars;
        let mut line = format!("{} pillars", n.count);
        if n.lots > 0 {
            line.push_str(&format!(", {} empty lot{} taken", n.lots, if n.lots == 1 { "" } else { "s" }));
        }
        ui.label(egui::RichText::new(line).small()).on_hover_text("Pillars stand about 24 m apart, shifted along the road to clear the roads and buildings under it (at most 42 m between supports).");
        if !n.buyouts.is_empty() {
            ui.label(egui::RichText::new(format!("Pillars buy out: {}", n.buyouts.join(", "))).color(egui::Color32::from_rgb(255, 170, 40)).small());
        }
        if let Some(why) = &n.refusal {
            ui.label(egui::RichText::new(format!("! Cannot be built: {why}.")).color(WARN).small());
        }
    }
    for w in &tools.grade_warnings {
        ui.label(egui::RichText::new(format!("! {w}")).color(WARN).small());
    }
    match tools.kind {
        RoadKind::Path => hint(ui, help::ROAD_PATH),
        RoadKind::Rail => hint(ui, help::ROAD_RAIL),
        RoadKind::Street => {}
    }
    hint(ui, help::tool_hint(Tool::Road));
}

fn zone(ui: &mut egui::Ui, sim: &Sim, tools: &mut ToolState, assets: &GameAssets) {
    // The land use is picked in the Build window (Zoning, Parks).
    ui.horizontal_wrapped(|ui| {
        let colour = zone_color32(assets.palette.zone_color(tools.land_use));
        let text = if tools.unzone { egui::RichText::new("Unzoning").strong() } else { egui::RichText::new(format!("Zoning: {}", tools.land_use.label())).strong().color(colour) };
        ui.label(text);
        if tools.zone_mode != ZoneMode::Polygon && ui.selectable_label(tools.unzone, "Unzone").on_hover_text("Left button erases parcels back to unzoned (the right button always does)").clicked() {
            tools.unzone = !tools.unzone;
        }
    });
    // Industrial land: the sub-zone painting sets.
    if tools.land_use == LandUse::Industrial && !tools.unzone {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("sub-zone").small());
            for (key, label) in nd_sim::world::INDUSTRY_CHOICES {
                if ui.selectable_label(tools.industry == key, label).clicked() {
                    tools.industry = key;
                }
            }
        });
    }
    ui.horizontal(|ui| {
        for m in ZoneMode::ALL {
            if ui.selectable_label(tools.zone_mode == m, m.label()).clicked() {
                tools.zone_mode = m;
                tools.points.clear();
                tools.stroke = None;
            }
        }
        if tools.zone_mode == ZoneMode::Brush {
            ui.separator();
            ui.add(egui::Slider::new(&mut tools.brush_radius, BRUSH_MIN..=BRUSH_MAX).text("brush m"));
        }
    });
    hint(ui, match tools.zone_mode {
        ZoneMode::Polygon => help::tool_hint(Tool::Zone),
        ZoneMode::Brush => help::ZONE_BRUSH,
        ZoneMode::Marquee => help::ZONE_MARQUEE,
    });
    // The parcel under the pointer: what it is, and whether it takes the
    // selected use.
    if let Some(p) = tools.hover_parcel.and_then(|i| sim.world.parcels.parcels.get(i)) {
        let built = p.land_use.map_or("unzoned", |l| l.label());
        let planned = tools.hover_parcel.and_then(|i| sim.world.planned_zoning().get(i).copied()).flatten();
        let plan_note = if planned != p.land_use { format!(" → {} planned", planned.map_or("unzoned", |l| l.label())) } else { String::new() };
        let front = if p.rough { "rough ground" } else if p.street_frontage { "street frontage" } else if p.path_frontage { "footpath frontage" } else { "landlocked backland" };
        let verdict = if tools.unzone { String::new() } else {
            match tools.hover_parcel.and_then(|i| sim.world.parcel_refusal(i, Some(tools.land_use))).or_else(|| p.refusal(tools.land_use)) {
                None => format!(" · takes {}", tools.land_use.label()),
                Some(why) => format!(" · no {}: {why}", tools.land_use.label()),
            }
        };
        hint(ui, &format!("Parcel {:.0} m² · {front} · {built}{plan_note}{verdict}", p.area));
        if tools.land_use == LandUse::Industrial {
            let i = tools.hover_parcel.unwrap_or_default();
            let ground = sim.world.suggested_industry(i).map_or_else(String::new, |k| format!("the ground suggests {k}"));
            let set = sim.world.industry_choice_at(p.center).map_or_else(String::new, |k| format!(" · sub-zone set: {k}"));
            if !ground.is_empty() || !set.is_empty() {
                hint(ui, &format!("{ground}{set}"));
            }
        }
    }
}

fn edit(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState, plan: &mut EffectivePlan) {
    hint(ui, help::tool_hint(Tool::Edit));
    let Some(id) = tools.selected_gesture else { return };
    match plan.sync(&sim.world).gestures.get(&id).cloned() {
        Some(Gesture::Road { points, lanes_fwd, lanes_bwd, curved, kind, mut elevation }) => {
            section(ui, "ROAD UPGRADE");
            let (mut f, mut b, mut c, mut k) = (lanes_fwd, lanes_bwd, curved, kind);
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                for (label, pf, pb, pk) in ROAD_PRESETS {
                    let on = k == pk && (pk != RoadKind::Street || (f == pf && b == pb));
                    if icons::selectable(ui, on, label, |p, r, c| icons::road(p, r, pk, pf, pb, c)).on_hover_text(preset_hint(label)).clicked() {
                        k = pk;
                        f = pf;
                        b = pb;
                        changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                changed |= ui.add(egui::Slider::new(&mut f, 0..=4).text("lanes fwd")).changed();
                changed |= ui.add(egui::Slider::new(&mut b, 0..=4).text("lanes back")).changed();
                if ui.button("Flip direction").clicked() {
                    std::mem::swap(&mut f, &mut b);
                    changed = true;
                }
                changed |= ui.checkbox(&mut c, "Curved").changed();
                // The whole road up or down a step (ends on other roads too).
                let lv = |e: &Vec<f32>, i: usize| e.get(i).copied().unwrap_or(0.0);
                if ui.small_button("raise").on_hover_text("Every point 2 m higher").clicked() {
                    elevation = (0..points.len()).map(|i| lv(&elevation, i) + crate::tools::ELEVATION_STEP).collect();
                    changed = true;
                }
                if ui.small_button("lower").on_hover_text("Every point 2 m lower").clicked() {
                    elevation = (0..points.len()).map(|i| lv(&elevation, i) - crate::tools::ELEVATION_STEP).collect();
                    changed = true;
                }
                if ui.button("Deselect").clicked() {
                    tools.selected_gesture = None;
                }
            });
            if changed {
                if f + b == 0 && k == RoadKind::Street {
                    f = 1;
                }
                if elevation.iter().all(|l| *l == 0.0) {
                    elevation.clear();
                }
                sim.world.set_gesture(id, Gesture::Road { points, lanes_fwd: f, lanes_bwd: b, curved: c, kind: k, elevation }, true);
                tools.status = "Road changed. Ctrl+Enter implements; buildings along it stay.".into();
            }
        }
        Some(Gesture::Roundabout { center, radius, lanes }) => {
            section(ui, "ROUNDABOUT");
            let (mut d, mut n) = (radius * 2.0, lanes);
            let mut changed = false;
            ui.horizontal(|ui| {
                changed |= ui.add(egui::Slider::new(&mut d, nd_sim::roads::RING_MIN_RADIUS * 2.0..=nd_sim::roads::RING_MAX_RADIUS * 2.0).step_by(2.0).suffix(" m").text("across")).changed();
                for k in [1u8, 2] {
                    if ui.selectable_label(n == k, format!("{k} lane{}", if k == 1 { "" } else { "s" })).clicked() && n != k {
                        n = k;
                        changed = true;
                    }
                }
                if ui.button("Deselect").clicked() {
                    tools.selected_gesture = None;
                }
            });
            if changed {
                sim.world.set_gesture(id, Gesture::Roundabout { center, radius: d * 0.5, lanes: n }, true);
                tools.status = "Roundabout changed. Ctrl+Enter implements.".into();
            }
        }
        Some(_) => hint(ui, help::EDIT_ZONE_SELECTED),
        None => tools.selected_gesture = None,
    }
}

fn transit(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState) {
    // Bus lines, metro lines and depots are picked in the Build window's
    // Transit section.
    ui.checkbox(&mut tools.show_routes, "Always show bus lines");
    let t = &sim.world.economy.transit;
    if t.depots == 0 {
        ui.label(egui::RichText::new(format!("No depot: buses cannot run. Place one (Build window, Transit, Bus depot) on a vacant lot by a street ({:.0} to build, {:.0}/day, {} buses).", nd_sim::transit::DEPOT_COST, nd_sim::transit::DEPOT_UPKEEP_PER_DAY, nd_sim::transit::DEPOT_CAPACITY)).small().color(egui::Color32::from_rgb(255, 160, 90)));
    } else {
        hint(ui, &format!("{} depot{} for {} buses · {} out · {} waiting · {} riding · {} riders today · fares {:.0} · upkeep {:.0}/day", t.depots, if t.depots == 1 { "" } else { "s" }, t.depot_capacity, t.buses, t.waiting, t.riding, t.riders_today, t.fares_today, t.upkeep_today));
    }
    match tools.transit_mode {
        TransitMode::Route => {
            hint(ui, help::tool_hint(Tool::Transit));
            let ids: Vec<nd_sim::RouteId> = sim.world.transit.routes.keys().collect();
            let mut remove: Option<nd_sim::RouteId> = None;
            let mut set_buses: Option<(nd_sim::RouteId, u8)> = None;
            // The list scrolls past three lines so the bar keeps its height.
            egui::ScrollArea::vertical().max_height(66.0).show(ui, |ui| {
                for id in ids {
                    let r = &sim.world.transit.routes[id];
                    route_row(ui, tools, id, r, &mut set_buses, &mut remove);
                }
            });
            if let Some((id, n)) = set_buses {
                sim.world.set_route_buses(id, n);
            }
            if let Some(id) = remove {
                sim.world.remove_bus_route(id);
                if tools.route_sel == Some(id) {
                    tools.route_sel = None;
                }
            }
        }
        TransitMode::Depot => hint(ui, help::TRANSIT_DEPOT),
        TransitMode::Metro => metro(ui, sim, tools),
    }
}

/// [subway] The Metro mode: what it does and costs, the lines with their
/// timetable and riders, and the line being laid.
fn metro(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState) {
    use nd_sim::subway as m;
    let s = &sim.world.subway.stats;
    hint(ui, &format!("Click lots to put stations there ({:.0} each, {:.0}/day) or pick standing ones and the Central Station, in order; Enter or right-click lays the line (tunnel {:.0}/m, {:.0}/km/day). Trains every {} min each way, 05:00 to midnight; fare {:.1}. U shows the tunnels.", m::STATION_COST, m::STATION_UPKEEP_PER_DAY, m::TUNNEL_COST_PER_M, m::TUNNEL_UPKEEP_PER_KM, m::DEFAULT_HEADWAY, m::FARE));
    hint(ui, &format!("{} station{} · {} line{} · {} train{} · {} waiting · {} riding · {} riders today ({} yesterday) · fares {:.0}", s.stations, if s.stations == 1 { "" } else { "s" }, s.lines, if s.lines == 1 { "" } else { "s" }, s.trains, if s.trains == 1 { "" } else { "s" }, s.waiting, s.riding, s.riders_today, s.riders_yesterday, s.fares_today));
    if !tools.metro_draft.is_empty() {
        let names: Vec<String> = tools.metro_draft.iter().filter_map(|b| sim.world.buildings.get(*b).map(|b| b.name.clone())).collect();
        hint(ui, &format!("laying: {}", names.join(" → ")));
    }
    let ids: Vec<nd_sim::MetroLineId> = sim.world.subway.lines.keys().collect();
    let mut remove = None;
    let mut headway = None;
    egui::ScrollArea::vertical().max_height(66.0).show(ui, |ui| {
        for id in ids {
            let l = &sim.world.subway.lines[id];
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{} · {} stations · {:.1} km · every {} min · {} riders today", l.name, l.stations.len(), l.length() / 1000.0, l.headway, l.riders_today)).small());
                if ui.small_button("−").on_hover_text("trains more often").clicked() {
                    headway = Some((id, l.headway.saturating_sub(2)));
                }
                if ui.small_button("+").on_hover_text("trains less often").clicked() {
                    headway = Some((id, l.headway + 2));
                }
                if ui.small_button("delete").clicked() {
                    remove = Some(id);
                }
            });
        }
    });
    if let Some((id, h)) = headway {
        let _ = sim.world.set_metro_headway(id, h);
    }
    if let Some(id) = remove {
        sim.world.remove_metro_line(id);
    }
}

/// One bus line: select it, add or remove a bus, delete it.
fn route_row(ui: &mut egui::Ui, tools: &mut ToolState, id: nd_sim::RouteId, r: &nd_sim::transit::BusRoute, set_buses: &mut Option<(nd_sim::RouteId, u8)>, remove: &mut Option<nd_sim::RouteId>) {
    ui.horizontal(|ui| {
        let sel = tools.route_sel == Some(id);
        if ui.selectable_label(sel, format!("{} · {} stops · {} bus{} · {} riders today{}", r.name, r.stops.len(), r.buses, if r.buses == 1 { "" } else { "es" }, r.riders_today, if r.broken.is_empty() { "" } else { " · NO ROAD between some stops" })).clicked() {
            tools.route_sel = if sel { None } else { Some(id) };
        }
        if ui.small_button("−").clicked() && r.buses > 0 {
            *set_buses = Some((id, r.buses - 1));
        }
        if ui.small_button("+").clicked() {
            *set_buses = Some((id, r.buses + 1));
        }
        if ui.small_button("delete").clicked() {
            *remove = Some(id);
        }
        // [lines] Stop by stop: ridership, times between stops, buses.
        if ui.small_button("stops…").on_hover_text("Open the line in the Lines window").clicked() {
            tools.route_sel = Some(id);
            super::Info::Lines.set_open(ui.ctx(), true);
        }
    });
}

fn rail(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState) {
    // The piece is picked in the Build window's Rail section.
    tools.dock = None;
    ui.label(egui::RichText::new(format!("Placing: {}", tools.rail_kind.label())).strong());
    let r = &sim.world.rail.stats;
    let (w, d) = nd_sim::Facility::size(tools.rail_kind);
    hint(ui, &match tools.rail_kind {
        RailFacility::Station => format!("A piece with track along one side and a street along the other, {w:.0} by {d:.0} m. Click anywhere: set beside track or a street it fuses to them, otherwise it lays stubs for you to join. {:.0} to build, {:.0}/day. People ride between stations for trips over {:.0} m; newcomers arrive by train.", nd_sim::rail::STATION_COST, nd_sim::rail::STATION_UPKEEP_PER_DAY, nd_sim::rail::RAIL_TRIP_MIN),
        RailFacility::Central => format!("The city's grand terminus, {w:.0} by {d:.0} m: intercity platforms under a vaulted glass hall, a metro station below (lay metro lines to it with Transit, Metro lines) and a bus terminal at its front. People walk {:.0} m to it. One per city; {:.0} to build, {:.0}/day.", nd_sim::rail::CENTRAL_REACH, nd_sim::rail::CENTRAL_COST, nd_sim::rail::CENTRAL_UPKEEP_PER_DAY),
        RailFacility::FreightYard => format!("A yard with sidings along one side and a street for lorries along the other, {w:.0} by {d:.0} m, anywhere you like. {:.0} to build, {:.0}/day. Once connected, imports come by rail at {:.0}% of the price and exports earn {:.0}% more.", nd_sim::rail::YARD_COST, nd_sim::rail::YARD_UPKEEP_PER_DAY, nd_sim::rail::RAIL_IMPORT_FACTOR * 100.0, (nd_sim::rail::RAIL_EXPORT_BONUS - 1.0) * 100.0),
    });
    hint(ui, help::tool_hint(Tool::Rail));
    hint(ui, &format!("{} station{} · {} yard{} · {} train{} out · next passenger train in {} min · {} waiting · {} riding · {} riders today · fares {:.0} · {:.0} units of freight in today, {:.0} waiting at yards · upkeep {:.0}/day", r.stations, if r.stations == 1 { "" } else { "s" }, r.yards, if r.yards == 1 { "" } else { "s" }, r.trains, if r.trains == 1 { "" } else { "s" }, r.next_passenger_in, r.waiting, r.riding, r.riders_today, r.fares_today, r.freight_in_today, r.freight_waiting, r.upkeep_today));
}

/// The Buildings tool: a row of sub-categories, the items in the open one,
/// a tier picker for services and attractions, and what it costs and does.
fn buildings(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState) {
    // The item is picked in the Build window; the tier is a parameter.
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(format!("Placing: {}", tools.build.menu_label())).strong());
        if tools.build.has_tiers() {
            ui.separator();
            for t in Tier::ALL {
                if ui.selectable_label(tools.build_tier == t, tools.build.with_tier(t).label()).clicked() {
                    tools.build_tier = t;
                }
            }
        }
    });
    placeable_info(ui, sim, tools.build_item());
    if tools.build_cat == BuildCategory::Services {
        let s = &sim.world.services.stats;
        hint(ui, &format!("Coverage of homes: police {:.0}% · fire {:.0}% · health {:.0}% · schools {:.0}% · {} incidents today · avg response {:.0} min · {}/{} units out · {} fires burning", s.coverage[0] * 100.0, s.coverage[1] * 100.0, s.coverage[2] * 100.0, s.coverage[3] * 100.0, s.incidents_today.iter().sum::<u32>(), s.avg_response(), s.units_busy, s.units, s.fires_burning));
    }
    hint(ui, help::tool_hint(Tool::Buildings));
}

/// What a placeable costs and does, in a line or two; for docks how the
/// docks are doing.
pub fn placeable_info(ui: &mut egui::Ui, sim: &mut Sim, what: Placeable) {
    let mut line = format!("{} · costs {:.0} to build, {:.0}/day to run, {} jobs", what.label(), what.cost(), what.upkeep(), what.jobs());
    if what.min_area() > 0.0 {
        line += &format!(", needs {:.0} m²", what.min_area());
    }
    match what {
        Placeable::Plant(k) => {
            line += help::power_note(k);
            line += &format!(" Goes {}.", k.site_hint());
        }
        Placeable::Service(k, t) => {
            if k.radius(t) > 0.0 {
                line += &format!(", covers {:.0} m {}", k.radius(t), if matches!(k, nd_sim::ServiceKind::TrashDepot | nd_sim::ServiceKind::ParkingEnforcement) { "around" } else { "by road" });
            }
            if k.units(t) > 0 {
                line += &format!(", {} {}{}", k.units(t), k.unit_kind().map_or("", |u| u.label()), if k.units(t) == 1 { "" } else { "s" });
            }
            let cap = k.capacity(t);
            if cap > 0 {
                line += &format!(", {cap} {}", match k { ServiceKind::Health => "beds", ServiceKind::Stadium => "seats", ServiceKind::Jail => "cells", _ => "places" });
            }
            line += ". ";
            line += help::service_note(k);
        }
        Placeable::Battery(k) => {
            line += &format!(
                ". Stores {:.0} MWh and charges or delivers up to {:.0} MW: it soaks up the plants' surplus before any is exported and covers shortfalls before power is imported; about {:.0}% comes back out. Any land but parks.",
                k.capacity_mwh(),
                k.max_mw(),
                nd_sim::power::BATTERY_CHARGE_EFF * nd_sim::power::BATTERY_DISCHARGE_EFF * 100.0
            );
        }
        Placeable::BusDepot => line += &format!(". {}", help::TRANSIT_DEPOT),
        Placeable::Rail(_) => line += ". Set beside track or a street it fuses to them; otherwise it lays stubs to join.",
        Placeable::Dock(_) => {}
        Placeable::MetroStation => line += &format!(". An entrance on its plaza, the platform underground; join stations into lines with Transit, Metro lines (tunnel {:.0}/m). Lifts land value round it.", nd_sim::subway::TUNNEL_COST_PER_M),
        Placeable::ParkingLot(s) => line += &format!(", {} spaces. Run by the city: drivers pay {:.1} a visit to the treasury (set the fee in the inspector or with the API).", s.capacity(), s.default_price()),
        // [tolls]
        Placeable::Toll(s) => {
            line += &format!(
                ". Click a road: it sits across the carriageway (both ways) on a straight run of {:.0} m or more. Every car pays {:.1} (lorries double, buses and emergencies free) and slows to {:.0} km/h through it; drivers weigh the toll and some take another way. Set prices in the Taxes window.",
                s.clearance() * 2.0 + 2.0,
                s.default_price(),
                s.speed() * 3.6
            );
            let t = &sim.world.tolls;
            if !t.booths.is_empty() {
                line += &format!(" {} booths: {} cars paid today, {:.0} taken.", t.booths.len(), t.paid_today, t.revenue_today());
            }
        }
        Placeable::CargoHub => {
            line += &format!(
                ". Freight terminal: businesses within {:.0} m import through it, by sea or rail when a port or yard works (one within {:.0} m unloads straight in), else in shared semis from the edge instead of one lorry each; its own lorries deliver on and park in its bays.",
                nd_sim::cargo_hub::HUB_REACH,
                nd_sim::cargo_hub::SIDING_REACH
            )
        }
    }
    hint(ui, &line);
    if let Placeable::Dock(k) = what {
        dock_hint(ui, sim, k);
    }
}

/// What a dock is and how the docks are doing (moved here from the Rail tool).
fn dock_hint(ui: &mut egui::Ui, sim: &mut Sim, k: nd_sim::PortKind) {
        let (w, d) = k.size();
        let s = &sim.world.ports.stats;
        hint(ui, &match k {
            nd_sim::PortKind::ShippingPort => format!("A container port, {w:.0} by {d:.0} m, on a river at least {:.0} m wide or on the sea; it builds a pier out into the water and a street along its landward side. {:.0} to build, {:.0}/day. Imports come by ship at {:.0}% of the price, exports earn {:.0}% more.", k.min_width(), k.cost(), k.upkeep_per_day(), nd_sim::ports::SHIP_IMPORT_FACTOR * 100.0, (nd_sim::ports::SHIP_EXPORT_BONUS - 1.0) * 100.0),
            nd_sim::PortKind::FerryTerminal => format!("A ferry terminal, {w:.0} by {d:.0} m, on any water; ferries run to every other terminal on connected water, or out to elsewhere. {:.0} to build, {:.0}/day, fare {:.0}.", k.cost(), k.upkeep_per_day(), nd_sim::ports::FERRY_FARE),
        });
        hint(ui, &format!("{} port{} · {} terminal{} · {} ship{} out · {} loads landed today, {} waiting · {} ferry riders today, fares {:.0}", s.ports, if s.ports == 1 { "" } else { "s" }, s.terminals, if s.terminals == 1 { "" } else { "s" }, s.ships_out, if s.ships_out == 1 { "" } else { "s" }, s.freight_landed_today, s.waiting_cargo, s.ferry_riders_today, s.ferry_fares_today));
}

/// Tooltip for a road preset button.
/// The view's layer: Underground, Ground, Sky [U cycles].
pub fn depth_switch(ui: &mut egui::Ui, tools: &mut ToolState) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("View [U]").small());
        for d in crate::tools::Depth::ALL {
            if ui.selectable_label(tools.depth == d, d.label()).clicked() {
                tools.depth = d;
            }
        }
        if tools.depth == crate::tools::Depth::Sky {
            ui.label(egui::RichText::new("air routes: coming soon").small().italics());
        }
    });
}

/// What the Road tool draws now, for the options header.
pub fn road_label(tools: &ToolState) -> String {
    if tools.roundabout {
        return "Roundabout".into();
    }
    if tools.tunnel {
        return format!("Tunnel, {} + {} lanes", tools.lanes_fwd, tools.lanes_bwd);
    }
    match tools.kind {
        RoadKind::Path => "Path".into(),
        RoadKind::Rail => "Rail track".into(),
        RoadKind::Street => {
            let preset = ROAD_PRESETS.iter().find(|(_, f, b, k)| *k == RoadKind::Street && *f == tools.lanes_fwd && *b == tools.lanes_bwd).map(|p| p.0);
            match preset {
                Some(name) => name.to_string(),
                None => format!("Street, {} + {} lanes", tools.lanes_fwd, tools.lanes_bwd),
            }
        }
    }
}

pub fn preset_hint(label: &str) -> &'static str {
    match label {
        "Tunnel" => "A road 8 m under the ground: passes under streets, water and buildings without taking them; joins other tunnels; start or end it on a street to make a portal ramp. Switches to the underground view",
        "Path" => "Footpath: walkers and bikes only",
        "Rail" => "Railway track: trains only, never fronts lots",
        "Lane" => "One lane each way",
        "Street" => "Two lanes each way",
        "Avenue" => "Three lanes each way",
        "One-way" => "Two lanes, one direction",
        "Ramp" => "One lane, one direction: slip roads onto a highway",
        "Highway" => "Four lanes one way: 25 m/s, no frontage, grade-separated",
        "Roundabout" => "A one-way ring with give-way entries: click to stamp it over roads and junctions",
        _ => "",
    }
}

/// Colour for a figure that should worry the player.
pub fn warn_if(cond: bool, text: String) -> egui::RichText {
    let t = egui::RichText::new(text);
    if cond { t.color(WARN) } else { t }
}
