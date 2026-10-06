//! The info windows: draggable, closable `egui::Window`s opened from the top
//! bar. egui remembers each window's position in its own memory; whether a
//! window is open lives in the same memory, so the UI carries no state of
//! its own for them. Each window's body is one function below, ported from
//! the sections of the old side panel.

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy_egui::egui;
use nd_sim::economy::Flow;
use nd_sim::{LandUse, Resource as Good};

use super::tools::warn_if;
use super::{help, hint, neon, people, saves, section, zone_color32, UiCtx, GOLD, WARN};
use crate::camera::CameraMode;

/// The windows the top bar can open, in toggle order. `Saves` and `Help`
/// come after a separator in the bar, `Dev` after another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Info {
    City,
    Economy,
    Market,
    People,
    Power,
    Transit,
    /// 911 calls, live and on record (ui/emergencies.rs).
    Emergencies,
    Land,
    Saves,
    Help,
    Dev,
    /// [districts] Districts, their policies and figures; tourism.
    Districts,
}

impl Info {
    pub const ALL: [Info; 12] = [Info::City, Info::Economy, Info::Market, Info::People, Info::Power, Info::Transit, Info::Emergencies, Info::Land, Info::Districts, Info::Saves, Info::Help, Info::Dev];

    pub fn title(self) -> &'static str {
        match self {
            Info::City => "City",
            Info::Economy => "Economy",
            Info::Market => "Market",
            Info::People => "People",
            Info::Power => "Power",
            Info::Transit => "Transit",
            Info::Emergencies => "Emergencies",
            Info::Land => "Land",
            Info::Saves => "Saves",
            Info::Help => "Help",
            Info::Dev => "Dev",
            Info::Districts => "Districts",
        }
    }

    /// Default width; the windows resize, this is only where they start.
    fn width(self) -> f32 {
        match self {
            Info::People => 420.0,
            Info::Help => 440.0,
            Info::Emergencies => 480.0,
            Info::Dev => 300.0,
            _ => 380.0,
        }
    }

    fn id(self) -> egui::Id {
        egui::Id::new(("info_window", self.title()))
    }

    pub fn is_open(self, ctx: &egui::Context) -> bool {
        ctx.data(|d| d.get_temp::<bool>(self.id()).unwrap_or(false))
    }

    pub fn set_open(self, ctx: &egui::Context, open: bool) {
        ctx.data_mut(|d| d.insert_temp(self.id(), open));
    }
}

/// Every open window. `content` is the map area between the bars: windows
/// open in a cascade down its left side and cannot be dragged under a bar.
pub fn show_all(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    for (i, info) in Info::ALL.into_iter().enumerate() {
        if !info.is_open(ctx) {
            continue;
        }
        let mut open = true;
        let pos = content.left_top() + egui::vec2(12.0 + 28.0 * i as f32, 12.0 + 30.0 * i as f32);
        egui::Window::new(egui::RichText::new(info.title()).color(GOLD).strong())
            .id(info.id().with("window"))
            .open(&mut open)
            .default_pos(pos)
            .default_width(info.width())
            .constrain_to(content)
            .max_height(content.height() - 24.0)
            .vscroll(true)
            .show(ctx, |ui| match info {
                Info::City => city(ui, p),
                Info::Economy => economy(ui, p),
                Info::Market => market(ui, p),
                Info::People => people::window(ui, p),
                Info::Power => power(ui, p),
                Info::Transit => transit(ui, p),
                Info::Emergencies => super::emergencies::window(ui, p),
                Info::Land => land(ui, p),
                Info::Saves => saves::panel(ui, p.sim, p.tools, p.selection, p.plan, p.saves, p.flags),
                Info::Help => help_window(ui),
                Info::Dev => dev(ui, p),
                Info::Districts => super::districts::window(ui, p),
            });
        if !open {
            info.set_open(ctx, false);
        }
    }
}

/// A labelled figure on one line: the label weak, the value plain.
fn row(ui: &mut egui::Ui, label: &str, value: impl Into<egui::RichText>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).weak());
        ui.label(value.into());
    });
}

fn city(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let e = &w.economy;
    let sites = w.buildings.values().filter(|b| b.construction.is_some()).count();
    section(ui, "BUILDINGS & LOTS");
    row(ui, "buildings", format!("{}   under construction {}", w.buildings.len() - sites, sites));
    row(ui, "lots", format!("{} ({} vacant, {} unusable)", w.lots.len(), w.lots.values().filter(|l| l.building.is_none() && l.usable()).count(), w.lots.values().filter(|l| l.landlocked).count()));
    if sites > 0 {
        row(ui, "sites will add", format!("{} homes and {} jobs", e.homes_building, e.jobs_building));
    }
    section(ui, "HOMES & JOBS");
    row(ui, "homes", format!("{}/{}", e.homes_total - e.homes_free, e.homes_total));
    row(ui, "jobs", format!("{}/{}", e.jobs_total - e.jobs_free, e.jobs_total));
    row(ui, "waiting to move in", format!("{}", e.waiting_immigrants));
    section(ui, "STREETS");
    row(ui, "cars on the road", format!("{}   lanes {}   signalled junctions {}", w.traffic.cars.len(), w.traffic.net.lanes.len(), w.traffic.net.intersections.iter().filter(|i| i.signalized).count()));
    row(ui, "parking", format!("{}/{} spaces in {} lots   circling today {}   fees today {:.0}", e.parking_used, e.parking_capacity, e.parking_lots, e.parking_street_today, e.parking_fees_today));
    row(ui, "walking", format!("{} trips today   walk-in sales {:.0}   {} pedestrian nodes", e.walk_trips_today, e.walk_in_sales_today, w.walk.nodes.len()));
}

/// Treasury and yesterday's ledger as signed bars, biggest first.
fn economy(ui: &mut egui::Ui, p: &mut UiCtx) {
    let e = &p.sim.world.economy;
    section(ui, "TREASURY");
    row(ui, "balance", warn_if(e.treasury < 0.0, format!("{:.0}", e.treasury)).strong());
    row(ui, "taxes so far today", format!("{:.0}", e.tax_today));
    row(ui, "plant upkeep", format!("{:.0}/day   imported power {:.0} today", e.power.upkeep_today, e.power.import_cost_today));
    let today: f32 = e.ledger.today.iter().sum();
    row(ui, "net today so far", warn_if(today < 0.0, format!("{today:+.0}")));

    section(ui, "YESTERDAY'S LEDGER");
    let entries = e.ledger.yesterday_sorted();
    if entries.is_empty() {
        hint(ui, "No books yet: the ledger closes at midnight.");
        return;
    }
    let scale = entries.iter().map(|(_, x)| x.abs()).fold(1.0, f32::max);
    for (flow, x) in &entries {
        signed_bar(ui, flow.label(), *x, scale);
    }
    let net: f32 = entries.iter().map(|(_, x)| x).sum();
    ui.separator();
    signed_bar(ui, "net", net, scale);
    hint(ui, &format!("income tax 12% of wages · business tax 5% of revenue · {} flows tracked", Flow::ALL.len()));
}

/// One ledger line: label, a bar growing right for income and left for
/// spending from a centre axis, and the figure. Drawn with the painter so
/// the sign reads at a glance.
fn signed_bar(ui: &mut egui::Ui, label: &str, value: f32, scale: f32) {
    ui.horizontal(|ui| {
        ui.add_sized(egui::vec2(110.0, 16.0), egui::Label::new(egui::RichText::new(label).small()));
        let (resp, painter) = ui.allocate_painter(egui::vec2(160.0, 14.0), egui::Sense::hover());
        let rect = resp.rect;
        let mid = rect.center().x;
        painter.vline(mid, rect.y_range(), egui::Stroke::new(1.0, egui::Color32::from_gray(90)));
        let half = rect.width() / 2.0 - 2.0;
        let len = (value.abs() / scale * half).max(if value == 0.0 { 0.0 } else { 1.0 });
        let (x0, x1, col) = if value >= 0.0 { (mid, mid + len, egui::Color32::from_rgb(110, 200, 140)) } else { (mid - len, mid, WARN) };
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, rect.top() + 2.0), egui::pos2(x1, rect.bottom() - 2.0)), 1.0, col);
        ui.label(warn_if(value < 0.0, format!("{value:+.0}")).small());
    });
}

fn market(ui: &mut egui::Ui, p: &mut UiCtx) {
    let e = &p.sim.world.economy;
    let m = &e.market;
    section(ui, "TRADE");
    row(ui, "businesses", format!("{}   closed so far {}", m.businesses, m.closed_total));
    row(ui, "today", format!("{} deals   imports {:.0}   exports {:.0}", m.deals_today, m.imports_today, m.exports_today));
    row(ui, "meals", warn_if(m.hungry > 0, format!("{} sold today   {} hungry", m.meals_sold_today, m.hungry)));
    section(ui, "PRICES");
    hint(ui, "price · stock in town; red is scarce (over 125% of base)");
    egui::Grid::new("market_prices").num_columns(3).spacing(egui::vec2(12.0, 2.0)).striped(true).show(ui, |ui| {
        for r in [Good::Produce, Good::Grain, Good::Flour, Good::Meat, Good::Dairy, Good::Groceries, Good::BakedGoods, Good::Parts, Good::Electronics, Good::Goods] {
            let price = m.avg_price.get(&r).copied().unwrap_or(r.base_price());
            let stock = m.stock.get(&r).copied().unwrap_or(0.0);
            let hot = price > r.base_price() * 1.25;
            ui.label(egui::RichText::new(r.label()).small());
            ui.label(warn_if(hot, format!("{price:.1}")).small());
            ui.label(egui::RichText::new(format!("{stock:.0}")).small().weak());
            ui.end_row();
        }
    });
    section(ui, "DEMAND");
    hint(ui, "how quickly vacant lots of each use develop");
    for lu in LandUse::ALL {
        let col = zone_color32(p.assets.palette.zone_color(lu));
        ui.add(egui::ProgressBar::new(e.demand_for(lu)).text(lu.label()).fill(col.gamma_multiply(0.7)).desired_height(12.0));
    }
}

fn power(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let pw = &w.economy.power;
    section(ui, "GRID");
    ui.add(egui::ProgressBar::new(pw.served).text(format!("served {:.0}%", pw.served * 100.0)).fill(if pw.served < 0.98 { WARN.gamma_multiply(0.8) } else { egui::Color32::from_rgb(110, 200, 140).gamma_multiply(0.8) }).desired_height(14.0));
    row(ui, "supply", format!("{:.0} MW from {} plant{}   + {:.0} MW imported", pw.supply_mw, pw.plants, if pw.plants == 1 { "" } else { "s" }, pw.imported_mw));
    row(ui, "demand", format!("{:.0} MW", pw.demand_mw));
    row(ui, "in the dark", warn_if(pw.unpowered_buildings > 0, format!("{} buildings   {} brownout hours today", pw.unpowered_buildings, pw.brownout_hours_today)));
    section(ui, "COSTS");
    row(ui, "plant upkeep", format!("{:.0}/day", pw.upkeep_today));
    row(ui, "grid import", format!("{:.0} today ({:.0} MW at {:.1} each)", pw.import_cost_today, nd_sim::power::GRID_IMPORT_MW, nd_sim::power::GRID_IMPORT_PRICE));
    section(ui, "WEATHER");
    row(ui, w.weather.label(), format!("cloud {:.0}%   wind {:.0}%", w.weather.cloud * 100.0, w.weather.wind * 100.0));
    hint(ui, "Solar dims under cloud and stops at night; wind follows the wind. Coal and nuclear are steady.");
}

fn transit(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let t = &w.economy.transit;
    section(ui, "BUSES");
    if t.depots == 0 {
        ui.label(egui::RichText::new("No depot: buses cannot run. The Buses tool places one.").small().color(WARN));
    }
    row(ui, "depots", format!("{} for {} buses   {} out on the lines", t.depots, t.depot_capacity, t.buses));
    row(ui, "riders", format!("{} waiting   {} riding   {} today", t.waiting, t.riding, t.riders_today));
    row(ui, "money", format!("fares {:.0} today   upkeep {:.0}/day", t.fares_today, t.upkeep_today));
    if !w.transit.routes.is_empty() {
        hint(ui, "lines (the Buses tool adds and removes buses)");
        for (id, r) in &w.transit.routes {
            let sel = p.tools.route_sel == Some(id);
            let text = format!("{} · {} stops · {} bus{} · {} riders today{}", r.name, r.stops.len(), r.buses, if r.buses == 1 { "" } else { "es" }, r.riders_today, if r.broken.is_empty() { "" } else { " · NO ROAD between some stops" });
            if ui.selectable_label(sel, egui::RichText::new(text).small()).clicked() {
                p.tools.route_sel = if sel { None } else { Some(id) };
            }
        }
    }
    let r = &w.rail.stats;
    section(ui, "RAIL");
    row(ui, "network", format!("{} station{}   {} yard{}   {} train{} out", r.stations, if r.stations == 1 { "" } else { "s" }, r.yards, if r.yards == 1 { "" } else { "s" }, r.trains, if r.trains == 1 { "" } else { "s" }));
    row(ui, "passengers", format!("{} waiting   {} riding   {} today   next train in {} min", r.waiting, r.riding, r.riders_today, r.next_passenger_in));
    row(ui, "trains today", format!("{} passenger   {} freight", r.passenger_trains_today, r.freight_trains_today));
    row(ui, "freight", format!("{:.0} units in today   {:.0} waiting at yards", r.freight_in_today, r.freight_waiting));
    row(ui, "money", format!("fares {:.0} today   upkeep {:.0}/day", r.fares_today, r.upkeep_today));
}

/// Overlays, land value and class mix, and the lighting controls: the
/// window about how the ground reads.
fn land(ui: &mut egui::Ui, p: &mut UiCtx) {
    section(ui, "LAYERS");
    hint(ui, "L steps through the map layers one at a time; tick several to stack them.");
    for (name, label, key) in crate::render::Overlays::LAYERS {
        let text = if key.is_empty() { label.to_string() } else { format!("{label} [{key}]") };
        if let Some(flag) = p.overlays.flag(name) {
            ui.checkbox(flag, text);
        }
    }
    ui.checkbox(&mut p.tools.show_routes, "Bus lines whatever the tool");
    let e = &p.sim.world.economy;
    section(ui, "LAND VALUE");
    row(ui, "average", format!("{:.2}", e.avg_land_value));
    row(ui, "class mix", format!("low {} · middle {} · high {}", e.class_counts[0], e.class_counts[1], e.class_counts[2]));
    row(ui, "housing", format!("low {} · middle {} · high {}", e.tier_counts[0], e.tier_counts[1], e.tier_counts[2]));
    section(ui, "LIGHTING");
    let lighting = &mut *p.lighting;
    ui.checkbox(&mut lighting.follow_clock, "Follow the clock");
    if !lighting.follow_clock {
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut lighting.hour, 0.0..=24.0).text("hour").fixed_decimals(1));
            for (label, h) in [("dawn", 6.5), ("noon", 12.5), ("dusk", 18.5), ("night", 23.0)] {
                if ui.small_button(label).clicked() {
                    lighting.hour = h;
                }
            }
        });
    }
}

fn help_window(ui: &mut egui::Ui) {
    section(ui, "WINDOWS");
    ui.label(egui::RichText::new(help::WINDOWS).small());
    section(ui, "CAMERA");
    ui.label(egui::RichText::new(help::CAMERA_ORBIT).small());
    ui.label(egui::RichText::new(format!("Walk mode: {}", help::CAMERA_WALK)).small());
    section(ui, "KEYS");
    ui.label(egui::RichText::new(help::HOTKEYS).small());
    ui.label(egui::RichText::new(help::TOOL_KEYS).small());
    section(ui, "PROJECT");
    ui.label(egui::RichText::new(help::PROJECT_NONE).small());
    section(ui, "TOOLS");
    for (tool, text) in help::TOOL_HINTS {
        neon(ui, tool.label());
        ui.label(egui::RichText::new(text).small());
    }
    section(ui, "SAVES");
    ui.label(egui::RichText::new(help::SAVES).small());
    hint(ui, help::ASSETS_RELOAD);
}

/// Developer odds and ends: the demo seed, the mind backend, frame and mesh
/// figures, asset errors. Out of the way of play but one click off. (A
/// window rather than a menu: egui's menu popup would not open from a bar
/// laid out on the background layer.)
fn dev(ui: &mut egui::Ui, p: &mut UiCtx) {
    if *p.mode == CameraMode::Orbit {
        let has_buildings = !p.sim.world.buildings.is_empty();
        let label = if has_buildings { "Seed demo district anyway" } else { "Seed demo district [G]" };
        if ui.button(label).clicked() {
            p.sim.seed_demo_district();
            p.tools.status = "Demo district laid out around the centre".into();
        }
        if has_buildings {
            hint(ui, help::SEED_EXISTING);
        }
        ui.separator();
    }
    hint(ui, &format!("minds: {} · backend: {}", nd_sim::MindProvider::name(&p.sim.minds), p.sim.minds.backend_name()));
    let fps = p.diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
    hint(ui, &format!("{fps:.0} fps · {} ticks/frame · effective speed {:.0}%", p.sim.ticks_last_frame, p.sim.effective_speed * 100.0));
    hint(ui, &format!("{} building meshes · {}k triangles", p.stats.building_entities, (p.stats.street_triangles + p.stats.building_triangles) / 1000));
    if let Some(err) = &p.assets.last_error {
        ui.label(egui::RichText::new(err).small().color(egui::Color32::from_rgb(255, 60, 90)));
    }
    hint(ui, help::ASSETS_RELOAD);
}
