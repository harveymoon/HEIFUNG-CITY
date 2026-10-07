//! The expanded views of the City, Market, Power, Transit and Land windows
//! (Economy's is `ui/economy.rs`, People's `ui/people.rs`, Emergencies'
//! `ui/emergencies.rs`). Each puts the range picker and its charts over
//! time first, then a table with one row per thing (plant, line, good),
//! then the compact view's figures underneath, so nothing the summary
//! showed goes missing when a window grows.

use bevy::prelude::Vec2;
use bevy_egui::egui::{self, Color32};
use nd_sim::{BusinessKind, LandUse, PowerPlantKind, Resource as Good, Tier};

use super::pane::{self, Series, Span};
use super::tools::warn_if;
use super::{hint, section, zone_color32, Info, UiCtx, GOLD, IVORY, WARN};
use crate::tools::Selection;

const GOOD: Color32 = Color32::from_rgb(110, 200, 140);

// ── City ───────────────────────────────────────────────────────────────────

pub fn city(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::span_picker(ui, Info::City);
    let w = &p.sim.world;
    let e = &w.economy;
    let h = &e.history;
    ui.horizontal_wrapped(|ui| {
        pane::kpi(ui, "people", egui::RichText::new(format!("{}", w.citizens.len())).color(GOLD));
        pane::kpi(ui, "homes filled", egui::RichText::new(format!("{}/{}", e.homes_total - e.homes_free, e.homes_total)));
        pane::kpi(ui, "jobs filled", egui::RichText::new(format!("{}/{}", e.jobs_total - e.jobs_free, e.jobs_total)));
        pane::kpi(ui, "cars on the road", egui::RichText::new(format!("{}", w.traffic.cars.len())));
    });
    let days = pane::days(h, span);
    let labels = pane::day_labels(&days);
    section(ui, "HOMES AND JOBS");
    let series = [
        Series::new("homes", Color32::from_rgb(110, 200, 220), pane::col(&days, |d| d.homes as f32)),
        Series::new("homes filled", Color32::from_rgb(60, 130, 160), pane::col(&days, |d| d.homes_filled as f32)),
        Series::new("jobs", GOLD, pane::col(&days, |d| d.jobs as f32)),
        Series::new("jobs filled", Color32::from_rgb(170, 120, 50), pane::col(&days, |d| d.jobs_filled as f32)),
    ];
    pane::line_chart(ui, &labels, &series, 120.0, &|x| format!("{x:.0}"));
    legend(ui, &series);
    section(ui, "BUILDINGS");
    let series = [Series::new("standing", IVORY, pane::col(&days, |d| d.buildings as f32)), Series::new("building sites", GOLD, pane::col(&days, |d| d.sites as f32))];
    pane::line_chart(ui, &labels, &series, 90.0, &|x| format!("{x:.0}"));
    legend(ui, &series);
    section(ui, "TRAFFIC, LAST 72 HOURS");
    let hours = pane::hours(h);
    pane::line_chart(ui, &pane::hour_labels(&hours), &[Series::new("vehicles on the road", WARN, pane::hcol(&hours, |r| r.cars as f32))], 90.0, &|x| format!("{x:.0}"));
    ui.separator();
    super::windows::city(ui, p);
}

fn legend(ui: &mut egui::Ui, series: &[Series]) {
    pane::legend(ui, &series.iter().map(|s| (s.name.as_str(), s.color)).collect::<Vec<_>>());
}

// ── Market ─────────────────────────────────────────────────────────────────

pub fn market(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::span_picker(ui, Info::Market);
    let w = &p.sim.world;
    let e = &w.economy;
    let m = &e.market;
    ui.horizontal_wrapped(|ui| {
        pane::kpi(ui, "businesses", egui::RichText::new(format!("{}", m.businesses)).color(GOLD));
        pane::kpi(ui, "deals today", egui::RichText::new(format!("{}", m.deals_today)));
        pane::kpi(ui, "imports today", egui::RichText::new(format!("{:.0}", m.imports_today)).color(WARN));
        pane::kpi(ui, "exports today", egui::RichText::new(format!("{:.0}", m.exports_today)).color(GOOD));
        pane::kpi(ui, "hungry", warn_if(m.hungry > 0, format!("{}", m.hungry)));
    });
    let days = pane::days(&e.history, span);
    let labels = pane::day_labels(&days);
    section(ui, "TRADE PER DAY");
    let series = [Series::new("imports", WARN, pane::col(&days, |d| d.imports)), Series::new("exports", GOOD, pane::col(&days, |d| d.exports)), Series::new("building materials", Color32::from_gray(160), pane::col(&days, |d| d.materials))];
    pane::line_chart(ui, &labels, &series, 110.0, &|x| format!("{x:.0}"));
    legend(ui, &series);
    section(ui, "BUSINESSES AND DEALS");
    let series = [Series::new("businesses", GOLD, pane::col(&days, |d| d.businesses as f32)), Series::new("deals that day", Color32::from_rgb(110, 200, 220), pane::col(&days, |d| d.deals as f32))];
    pane::line_chart(ui, &labels, &series, 90.0, &|x| format!("{x:.0}"));
    legend(ui, &series);
    section(ui, "DEMAND FOR NEW BUILDINGS");
    let series: Vec<Series> = LandUse::ALL.iter().map(|&lu| Series::new(lu.label(), zone_color32(p.assets.palette.zone_color(lu)), pane::col(&days, |d| d.demand[lu.index()] * 100.0))).collect();
    pane::line_chart(ui, &labels, &series, 100.0, &|x| format!("{x:.0}%"));
    legend(ui, &series);

    section(ui, "EVERY GOOD");
    hint(ui, "price against its base, and the stock in town; red is scarce (over 125% of base)");
    egui::Grid::new("market_prices_all").num_columns(5).spacing(egui::vec2(14.0, 2.0)).striped(true).show(ui, |ui| {
        for h in ["good", "price", "base", "vs base", "stock"] {
            ui.label(egui::RichText::new(h).small().weak());
        }
        ui.end_row();
        for r in Good::ALL {
            let price = m.avg_price.get(&r).copied().unwrap_or(r.base_price());
            let stock = m.stock.get(&r).copied().unwrap_or(0.0);
            let ratio = price / r.base_price().max(1e-3);
            ui.label(egui::RichText::new(r.label()).small());
            ui.label(warn_if(ratio > 1.25, format!("{price:.1}")).small());
            ui.label(egui::RichText::new(format!("{:.1}", r.base_price())).small().weak());
            ui.label(warn_if(ratio > 1.25, format!("{:.0}%", ratio * 100.0)).small());
            ui.label(egui::RichText::new(format!("{stock:.0}")).small().weak());
            ui.end_row();
        }
    });
    section(ui, "HOW EACH KIND OF BUSINESS PAYS");
    hint(ui, "takings over costs, smoothed over a few days; under 100% loses money");
    egui::Grid::new("business_cover").num_columns(4).spacing(egui::vec2(14.0, 2.0)).striped(true).show(ui, |ui| {
        let mut count = [0u32; BusinessKind::COUNT];
        let mut open = [0u32; BusinessKind::COUNT];
        for b in w.buildings.values() {
            if let Some(bz) = &b.business {
                count[bz.kind.index()] += 1;
                open[bz.kind.index()] += u32::from(!bz.closed);
            }
        }
        for k in BusinessKind::ALL {
            let i = k.index();
            if count[i] == 0 && e.cover_of(k).is_none() {
                continue;
            }
            ui.label(egui::RichText::new(k.label()).small());
            ui.label(egui::RichText::new(format!("{} open of {}", open[i], count[i])).small().weak());
            match e.cover_of(k) {
                Some(c) => {
                    ui.label(warn_if(c < 1.0, format!("{:.0}%", c * 100.0)).small());
                    let (r, _) = ui.allocate_exact_size(egui::vec2(100.0, 9.0), egui::Sense::hover());
                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width() * (c / 2.0).min(1.0), r.height())), 1.0, if c < 1.0 { WARN } else { GOOD });
                    ui.painter().vline(r.center().x, r.y_range(), egui::Stroke::new(1.0, Color32::from_gray(150)));
                }
                None => {
                    ui.label(egui::RichText::new("-").small().weak());
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });
}

// ── Power ──────────────────────────────────────────────────────────────────

/// A colour per kind of plant: coal soot, solar yellow, wind white, nuclear
/// green, hydro blue, tidal teal, geothermal red.
pub fn plant_color(k: PowerPlantKind) -> Color32 {
    match k {
        PowerPlantKind::Coal => Color32::from_rgb(130, 120, 115),
        PowerPlantKind::Solar => Color32::from_rgb(245, 210, 80),
        PowerPlantKind::Wind => Color32::from_rgb(210, 225, 235),
        PowerPlantKind::Nuclear => Color32::from_rgb(140, 230, 110),
        PowerPlantKind::Hydro => Color32::from_rgb(80, 150, 240),
        PowerPlantKind::Tidal => Color32::from_rgb(70, 200, 190),
        PowerPlantKind::Geothermal => Color32::from_rgb(230, 100, 70),
    }
}

const IMPORT: Color32 = Color32::from_rgb(200, 120, 230);
const EXPORT: Color32 = Color32::from_rgb(120, 225, 245);
const DEMAND: Color32 = Color32::from_rgb(255, 90, 90);
/// Batteries: delivering (above the axis) and their charge level.
pub const BATTERY: Color32 = Color32::from_rgb(90, 235, 170);
/// Batteries charging (below the axis).
const CHARGING: Color32 = Color32::from_rgb(40, 140, 110);

/// The plant table, compact or wide: a row per plant and battery with what
/// it puts out, what it costs to run and per MWh, and what it does to the
/// air and the ears; batteries show their charge and flow instead. The
/// figures come from `World::plant_rows`, the same as the API's.
pub fn plant_table(ui: &mut egui::Ui, w: &nd_sim::World, rows: &[nd_sim::PlantRow], wide: bool) -> Option<(nd_sim::BuildingId, Vec2)> {
    let mut act = None;
    let cols = if wide { 9 } else { 6 };
    egui::Grid::new(if wide { "power_plants_wide" } else { "power_plants" }).num_columns(cols).spacing(egui::vec2(10.0, 2.0)).striped(true).show(ui, |ui| {
        let heads: &[(&str, &str)] = if wide {
            &[("plant", ""), ("now", "output now / nameplate"), ("24 h mean", "mean output over the last 24 hours"), ("upkeep", "running cost per day"), ("cost/MWh", "upkeep over the last 24 hours' energy"), ("CO2 t/day", "carbon dioxide at the last 24 hours' rate"), ("smog", "air at the fence now, 0 to 1 (coal at full output is 1)"), ("noise", "at the fence while running"), ("", "")]
        } else {
            &[("plant", ""), ("now", "output now"), ("upkeep", "running cost per day"), ("cost/MWh", "upkeep over the last 24 hours' energy"), ("air", "CO2 per day; smog at the fence now"), ("noise", "at the fence while running")]
        };
        for (h, tip) in heads {
            let l = ui.label(egui::RichText::new(*h).small().weak());
            if !tip.is_empty() {
                l.on_hover_text(*tip);
            }
        }
        ui.end_row();
        for r in rows {
            let (color, kind) = match r.asset {
                nd_sim::GridAsset::Plant(k) => (plant_color(k), k.label().to_string()),
                nd_sim::GridAsset::Battery(k) => (BATTERY, k.label().to_string()),
            };
            let status = if r.pending { " (not yet working)" } else if r.derelict { " (derelict)" } else { "" };
            let name = ui.label(egui::RichText::new(&r.name).small().color(color));
            name.on_hover_text(format!("{kind}{status}"));
            let cost = r.cost_per_mwh.map_or("-".to_string(), |c| format!("{c:.1}"));
            let noise = r.noise_db.map_or("quiet".to_string(), |d| format!("{d:.0} dB"));
            match r.asset {
                nd_sim::GridAsset::Plant(_) => {
                    let pct = if r.capacity_mw > 0.0 { r.now_mw / r.capacity_mw * 100.0 } else { 0.0 };
                    let smog = egui::RichText::new(format!("{:.2}", r.air)).small();
                    ui.label(egui::RichText::new(if wide { format!("{:.1} MW · {pct:.0}% of {:.0}", r.now_mw, r.capacity_mw) } else { format!("{:.1} MW", r.now_mw) }).small());
                    if wide {
                        ui.label(egui::RichText::new(format!("{:.1} MW", r.mean_mw)).small().weak());
                    }
                    ui.label(egui::RichText::new(format!("{:.0}", r.upkeep_per_day)).small());
                    ui.label(egui::RichText::new(cost).small());
                    if wide {
                        ui.label(warn_if(r.co2_t_per_day > 100.0, format!("{:.0}", r.co2_t_per_day)).small());
                        ui.label(if r.air > 0.5 { smog.color(WARN) } else { smog });
                    } else {
                        ui.label(warn_if(r.co2_t_per_day > 100.0, if r.co2_t_per_day > 0.0 { format!("{:.0} t CO2 · smog {:.1}", r.co2_t_per_day, r.air) } else { "clean".to_string() }).small());
                    }
                    ui.label(warn_if(r.noise_db.is_some_and(|d| d >= 60.0), noise).small());
                }
                nd_sim::GridAsset::Battery(_) => {
                    let level = if r.capacity_mwh > 0.0 { r.charge_mwh / r.capacity_mwh } else { 0.0 };
                    let flow = if r.now_mw > 0.05 { format!("gives {:.1} MW", r.now_mw) } else if r.now_mw < -0.05 { format!("takes {:.1} MW", -r.now_mw) } else { "holding".to_string() };
                    ui.label(egui::RichText::new(if wide { format!("{:.0}% of {:.0} MWh · {flow}", level * 100.0, r.capacity_mwh) } else { format!("{:.0}% · {flow}", level * 100.0) }).small());
                    if wide {
                        ui.label(egui::RichText::new(format!("{:.0} MWh out", r.mean_mw * 24.0)).small().weak());
                    }
                    ui.label(egui::RichText::new(format!("{:.0}", r.upkeep_per_day)).small());
                    ui.label(egui::RichText::new(cost).small());
                    if wide {
                        ui.label(egui::RichText::new("0").small().weak());
                        ui.label(egui::RichText::new("0.00").small().weak());
                    } else {
                        ui.label(egui::RichText::new("clean").small().weak());
                    }
                    ui.label(egui::RichText::new(noise).small());
                }
            }
            if wide && ui.small_button("go").on_hover_text("Select it and fly there").clicked() {
                if let Some(l) = w.buildings.get(r.id).and_then(|b| w.lots.get(b.lot)) {
                    act = Some((r.id, Vec2::new(l.center.x, l.center.y)));
                }
            }
            ui.end_row();
        }
    });
    act
}

/// A plant's own record over the hours shown, padded at the front with
/// zeros when it is younger than the chart.
fn plant_hours(series: &std::collections::VecDeque<f32>, n: usize) -> Vec<f32> {
    let take = series.len().min(n);
    let mut v = vec![0.0; n - take];
    v.extend(series.iter().skip(series.len() - take));
    v
}

pub fn power(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::span_picker(ui, Info::Power);
    let act: Option<(nd_sim::BuildingId, Vec2)>;
    {
        let w = &p.sim.world;
        let pw = &w.economy.power;
        let h = &w.economy.history;
        ui.horizontal_wrapped(|ui| {
            pane::kpi(ui, "served", warn_if(pw.served < 0.98, format!("{:.0}%", pw.served * 100.0)).color(if pw.served < 0.98 { WARN } else { GOOD }));
            pane::kpi(ui, "supply now", egui::RichText::new(format!("{:.0} MW", pw.supply_mw)).color(GOLD));
            pane::kpi(ui, "demand now", egui::RichText::new(format!("{:.0} MW", pw.demand_mw)));
            pane::kpi(ui, "imported now", egui::RichText::new(format!("{:.0} MW", pw.imported_mw)).color(IMPORT));
            pane::kpi(ui, "exported now", egui::RichText::new(format!("{:.0} MW", pw.exported_mw)).color(EXPORT));
            if pw.batteries > 0 {
                let level = if pw.battery_capacity_mwh > 0.0 { pw.battery_charge_mwh / pw.battery_capacity_mwh } else { 0.0 };
                pane::kpi(ui, "batteries full", egui::RichText::new(format!("{:.0}%", level * 100.0)).color(BATTERY));
            }
            pane::kpi(ui, "in the dark", warn_if(pw.unpowered_buildings > 0, format!("{}", pw.unpowered_buildings)));
        });
        let used: Vec<PowerPlantKind> = PowerPlantKind::ALL.into_iter().filter(|k| h.hours.iter().any(|r| r.supply_by_kind[k.index()] > 0.0) || h.days.iter().any(|d| d.supply_mwh[k.index()] > 0.0)).collect();
        section(ui, "SUPPLY AND DEMAND, LAST 72 HOURS");
        let hours = pane::hours(h);
        let hour_labels = pane::hour_labels(&hours);
        // Above the axis what fed the city: each kind of plant, the
        // batteries giving back, the regional grid; below it where the
        // surplus went: into the batteries and out to the region.
        let mut series: Vec<Series> = used.iter().map(|k| Series::new(k.short(), plant_color(*k), pane::hcol(&hours, |r| r.supply_by_kind[k.index()]))).collect();
        series.push(Series::new("batteries delivering", BATTERY, pane::hcol(&hours, |r| r.battery_out_mw)));
        series.push(Series::new("imported", IMPORT, pane::hcol(&hours, |r| r.imported_mw)));
        series.push(Series::new("into batteries", CHARGING, pane::hcol(&hours, |r| -r.battery_in_mw)));
        series.push(Series::new("exported", EXPORT, pane::hcol(&hours, |r| -r.exported_mw)));
        series.retain(|s| s.values.iter().any(|v| *v != 0.0) || used.iter().any(|k| k.short() == s.name));
        let demand = Series::new("demand", DEMAND, pane::hcol(&hours, |r| r.demand_mw));
        pane::stacked_bars(ui, &hour_labels, &series, 160.0, false, Some(&demand), &|x| format!("{x:.1} MW"));
        let mut lg: Vec<(&str, Color32)> = series.iter().map(|s| (s.name.as_str(), s.color)).collect();
        lg.push(("demand (line)", DEMAND));
        pane::legend(ui, &lg);
        hint(ui, "Each hour's supply stacked by source above the line: the plants by kind, the batteries giving back, what the regional grid sold. Below the line, where a surplus went: into the batteries first, then sold to the region. The red line is what the city wanted.");

        // [grid-weather] What power cost and how hot it was, hour by hour.
        section(ui, "GRID PRICE AND TEMPERATURE, LAST 72 HOURS");
        let ratio = nd_sim::power::GRID_EXPORT_PRICE / nd_sim::power::GRID_IMPORT_PRICE;
        let series = [
            Series::new("import price /MWh", IMPORT, pane::hcol(&hours, |r| r.import_price)),
            Series::new("export price /MWh", EXPORT, pane::hcol(&hours, |r| r.import_price * ratio)),
        ];
        pane::line_chart(ui, &hour_labels, &series, 90.0, &|x| format!("{x:.2}"));
        legend(ui, &series);
        let temp = [Series::new("temperature °C", Color32::from_rgb(255, 200, 120), pane::hcol(&hours, |r| r.temp_c))];
        pane::line_chart(ui, &hour_labels, &temp, 60.0, &|x| format!("{x:.1} °C"));
        hint(ui, "The regional grid charges by its load: cheap in the small hours, dear in the evening peak, dearest when a heat wave has every air conditioner on. It pays a fifth of that for exports.");

        if h.hours.iter().any(|r| r.battery_capacity_mwh > 0.0) {
            section(ui, "BATTERIES, LAST 72 HOURS");
            let series = [
                Series::new("stored MWh", BATTERY, pane::hcol(&hours, |r| r.battery_charge_mwh)),
                Series::new("capacity MWh", Color32::from_gray(120), pane::hcol(&hours, |r| r.battery_capacity_mwh)),
                Series::new("charging MW", CHARGING, pane::hcol(&hours, |r| r.battery_in_mw)),
                Series::new("delivering MW", Color32::from_rgb(255, 190, 80), pane::hcol(&hours, |r| r.battery_out_mw)),
            ];
            pane::line_chart(ui, &hour_labels, &series, 110.0, &|x| format!("{x:.1}"));
            legend(ui, &series);
            hint(ui, "How full the batteries were at the end of each hour, and when they took surplus in or carried the city (about 90% of what goes in comes back out).");
        }

        let days = pane::days(h, if span == Span::Hours { Span::Week } else { span });
        section(ui, "ENERGY PER DAY");
        let mut series: Vec<Series> = used.iter().map(|k| Series::new(k.short(), plant_color(*k), pane::col(&days, |d| d.supply_mwh[k.index()]))).collect();
        series.push(Series::new("batteries delivering", BATTERY, pane::col(&days, |d| d.battery_out_mwh)));
        series.push(Series::new("imported", IMPORT, pane::col(&days, |d| d.imported_mwh)));
        series.push(Series::new("into batteries", CHARGING, pane::col(&days, |d| -d.battery_in_mwh)));
        series.push(Series::new("exported", EXPORT, pane::col(&days, |d| -d.exported_mwh)));
        series.retain(|s| s.values.iter().any(|v| *v != 0.0) || used.iter().any(|k| k.short() == s.name));
        let demand = Series::new("demand", DEMAND, pane::col(&days, |d| d.demand_mwh));
        pane::stacked_bars(ui, &pane::day_labels(&days), &series, 130.0, false, Some(&demand), &|x| format!("{x:.0} MWh"));
        section(ui, "MONEY FOR POWER PER DAY");
        let series = [
            Series::new("export income", EXPORT, pane::col(&days, |d| d.flow(nd_sim::economy::Flow::GridExport))),
            Series::new("import cost", IMPORT, pane::col(&days, |d| d.flow(nd_sim::economy::Flow::GridImport))),
            Series::new("upkeep", Color32::from_rgb(255, 120, 90), pane::col(&days, |d| d.flow(nd_sim::economy::Flow::PlantUpkeep))),
        ];
        pane::stacked_bars(ui, &pane::day_labels(&days), &series, 100.0, true, None, &|x| format!("{x:.0}"));
        legend(ui, &series);
        section(ui, "BROWNOUTS AND PEAK");
        let series = [Series::new("brownout hours", WARN, pane::col(&days, |d| d.brownout_hours as f32)), Series::new("peak demand MW", IVORY, pane::col(&days, |d| d.peak_demand_mw))];
        pane::line_chart(ui, &pane::day_labels(&days), &series, 90.0, &|x| format!("{x:.0}"));
        legend(ui, &series);

        section(ui, "PLANTS AND BATTERIES");
        let rows = w.plant_rows();
        if rows.is_empty() {
            hint(ui, "No plants: the city buys what the regional grid will sell. Buildings, Power places plants and battery packs.");
        }
        act = plant_table(ui, w, &rows, true);
        if !rows.is_empty() {
            section(ui, "EACH PLANT, LAST 72 HOURS");
            let n = hours.len();
            for r in &rows {
                let Some(log) = h.plant(r.id) else { continue };
                if n == 0 {
                    break;
                }
                match r.asset {
                    nd_sim::GridAsset::Plant(k) => {
                        ui.label(egui::RichText::new(format!("{} · {}", r.name, k.label())).small().color(plant_color(k)));
                        let s = [Series::new("output MW", plant_color(k), plant_hours(&log.hours, n)), Series::new("nameplate", Color32::from_gray(90), vec![r.capacity_mw; n])];
                        pane::line_chart(ui, &hour_labels, &s, 56.0, &|x| format!("{x:.1} MW"));
                    }
                    nd_sim::GridAsset::Battery(k) => {
                        ui.label(egui::RichText::new(format!("{} · {}", r.name, k.label())).small().color(BATTERY));
                        let s = [Series::new("stored MWh", BATTERY, plant_hours(&log.level, n)), Series::new("flow MW (+ gives, - takes)", Color32::from_rgb(255, 190, 80), plant_hours(&log.hours, n))];
                        pane::line_chart(ui, &hour_labels, &s, 56.0, &|x| format!("{x:.1}"));
                    }
                }
            }
        }
    }
    if let Some((b, at)) = act {
        *p.selection = Selection::Building(b);
        p.fly_to = Some(at);
    }
    ui.separator();
    super::windows::power(ui, p);
}

// ── Transit ────────────────────────────────────────────────────────────────

pub fn transit(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::span_picker(ui, Info::Transit);
    let mut pick: Option<Option<nd_sim::RouteId>> = None;
    {
        let w = &p.sim.world;
        let e = &w.economy;
        let h = &e.history;
        let l = &e.ledger;
        use nd_sim::economy::Flow;
        let fares = l.yesterday_of(Flow::BusFares) + l.yesterday_of(Flow::RailFares);
        let upkeep = l.yesterday_of(Flow::TransitUpkeep) + l.yesterday_of(Flow::RailUpkeep);
        ui.horizontal_wrapped(|ui| {
            pane::kpi(ui, "bus lines", egui::RichText::new(format!("{}", w.transit.routes.len())).color(GOLD));
            pane::kpi(ui, "buses out", egui::RichText::new(format!("{}", e.transit.buses)));
            pane::kpi(ui, "riders today", egui::RichText::new(format!("{}", e.transit.riders_today + w.rail.stats.riders_today)));
            pane::kpi(ui, "fares yesterday", egui::RichText::new(format!("{fares:.0}")).color(GOOD));
            pane::kpi(ui, "upkeep yesterday", egui::RichText::new(format!("{upkeep:.0}")).color(WARN));
        });
        let days = pane::days(h, if span == Span::Hours { Span::Week } else { span });
        let labels = pane::day_labels(&days);
        section(ui, "RIDERS PER DAY BY MODE");
        let series = [
            Series::new("bus", GOLD, pane::col(&days, |d| d.bus_riders as f32)),
            Series::new("rail", Color32::from_rgb(150, 200, 255), pane::col(&days, |d| d.rail_riders as f32)),
            Series::new("ferry", Color32::from_rgb(70, 200, 190), pane::col(&days, |d| d.ferry_riders as f32)),
            Series::new("air taxi", Color32::from_rgb(200, 120, 230), pane::col(&days, |d| d.air_riders as f32)),
        ];
        pane::stacked_bars(ui, &labels, &series, 120.0, false, None, &|x| format!("{x:.0}"));
        legend(ui, &series);

        section(ui, "RIDERS PER DAY BY BUS LINE");
        let mut lines: Vec<(nd_sim::RouteId, String, f32)> = w.transit.routes.iter().map(|(id, r)| (id, r.name.clone(), days.iter().map(|d| d.riders_on(id) as f32).sum())).collect();
        lines.sort_by(|a, b| b.2.total_cmp(&a.2));
        let series: Vec<Series> = lines.iter().take(10).enumerate().map(|(i, (id, name, _))| Series::new(name.clone(), pane::palette(i), pane::col(&days, |d| d.riders_on(*id) as f32))).collect();
        if series.is_empty() {
            hint(ui, "No bus lines yet: the Buses tool draws them.");
        } else {
            pane::line_chart(ui, &labels, &series, 120.0, &|x| format!("{x:.0}"));
            legend(ui, &series);
        }

        section(ui, "FARES AGAINST UPKEEP PER DAY");
        let series: Vec<Series> = [Flow::BusFares, Flow::RailFares, Flow::TransitUpkeep, Flow::RailUpkeep].iter().map(|f| Series::new(f.label(), super::economy::flow_color(*f), pane::col(&days, |d| d.flow(*f)))).collect();
        pane::stacked_bars(ui, &labels, &series, 110.0, true, None, &|x| format!("{x:+.0}"));
        legend(ui, &series);

        section(ui, "ON THE MOVE, LAST 72 HOURS");
        let hours = pane::hours(h);
        let series = [Series::new("waiting for a bus", WARN, pane::hcol(&hours, |r| r.bus_waiting as f32)), Series::new("on a bus", GOLD, pane::hcol(&hours, |r| r.bus_riding as f32)), Series::new("on a train", Color32::from_rgb(150, 200, 255), pane::hcol(&hours, |r| r.rail_riding as f32))];
        pane::line_chart(ui, &pane::hour_labels(&hours), &series, 100.0, &|x| format!("{x:.0}"));
        legend(ui, &series);

        if !w.transit.routes.is_empty() {
            section(ui, "BUS LINES");
            hint(ui, "click a line to show it on the map");
            let fares_of = |id: nd_sim::RouteId| l.items_yesterday.iter().filter(|e| e.item == nd_sim::economy::Item::Route(id)).map(|e| e.amount).sum::<f32>();
            egui::Grid::new("transit_lines").num_columns(7).spacing(egui::vec2(12.0, 2.0)).striped(true).show(ui, |ui| {
                for hd in ["line", "stops", "buses", "today", "yesterday", "avg/day", "fares yesterday"] {
                    ui.label(egui::RichText::new(hd).small().weak());
                }
                ui.end_row();
                for (id, r) in &w.transit.routes {
                    let sel = p.tools.route_sel == Some(id);
                    if ui.selectable_label(sel, egui::RichText::new(&r.name).small()).clicked() {
                        pick = Some(if sel { None } else { Some(id) });
                    }
                    ui.label(egui::RichText::new(format!("{}", r.stops.len())).small());
                    ui.label(egui::RichText::new(format!("{}", r.buses)).small());
                    ui.label(egui::RichText::new(format!("{}", r.riders_today)).small());
                    ui.label(egui::RichText::new(format!("{}", days.last().map_or(0, |d| d.riders_on(id)))).small());
                    ui.label(egui::RichText::new(format!("{:.0}", days.iter().map(|d| d.riders_on(id) as f32).sum::<f32>() / days.len().max(1) as f32)).small());
                    ui.label(egui::RichText::new(format!("{:.0}", fares_of(id))).small());
                    ui.end_row();
                }
            });
        }
    }
    if let Some(sel) = pick {
        p.tools.route_sel = sel;
    }
    ui.separator();
    super::windows::transit(ui, p);
}

// ── Land ───────────────────────────────────────────────────────────────────

pub fn land(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::span_picker(ui, Info::Land);
    {
        let w = &p.sim.world;
        let days = pane::days(&w.economy.history, span);
        let labels = pane::day_labels(&days);
        section(ui, "LAND VALUE");
        pane::line_chart(ui, &labels, &[Series::new("average land value", GOLD, pane::col(&days, |d| d.avg_land_value))], 90.0, &|x| format!("{x:.2}"));
        section(ui, "RENT BY HOUSING TIER, PER DAY");
        let series: Vec<Series> = Tier::ALL.iter().enumerate().map(|(i, t)| Series::new(t.label(), super::people::tier_color(*t), pane::col(&days, |d| d.avg_rent[i]))).collect();
        pane::line_chart(ui, &labels, &series, 100.0, &|x| format!("{x:.0}"));
        legend(ui, &series);
        section(ui, "LOTS BY LAND USE");
        egui::Grid::new("land_lots").num_columns(4).spacing(egui::vec2(14.0, 2.0)).striped(true).show(ui, |ui| {
            for hd in ["land use", "lots", "built", "vacant"] {
                ui.label(egui::RichText::new(hd).small().weak());
            }
            ui.end_row();
            for lu in LandUse::ALL {
                // [free-placement] City ground is out of the zones: its own row below.
                let lots: Vec<&nd_sim::Lot> = w.lots.values().filter(|l| l.land_use == lu && !w.is_city_ground(l)).collect();
                if lots.is_empty() {
                    continue;
                }
                let built = lots.iter().filter(|l| l.building.is_some()).count();
                ui.label(egui::RichText::new(lu.label()).small().color(zone_color32(p.assets.palette.zone_color(lu))));
                ui.label(egui::RichText::new(format!("{}", lots.len())).small());
                ui.label(egui::RichText::new(format!("{built}")).small());
                ui.label(egui::RichText::new(format!("{}", lots.iter().filter(|l| l.building.is_none() && l.usable()).count())).small());
                ui.end_row();
            }
            let city = w.lots.values().filter(|l| w.is_city_ground(l)).count();
            if city > 0 {
                ui.label(egui::RichText::new("City buildings").small().color(zone_color32(p.assets.palette.zone_color(LandUse::Civic))));
                ui.label(egui::RichText::new(format!("{city}")).small());
                ui.label(egui::RichText::new(format!("{city}")).small());
                ui.label(egui::RichText::new("0").small());
                ui.end_row();
            }
        });
    }
    ui.separator();
    super::windows::land(ui, p);
}
