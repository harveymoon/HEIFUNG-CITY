//! City Data pages: City (Overview, Land and rent), Market (Goods and
//! prices, Businesses, Trade) and Services (Power, Transit). Economy's are
//! in `ui/economy.rs`, People's in `ui/people.rs`, Districts' and Tourism's
//! in `ui/districts.rs`, Emergencies' in `ui/emergencies.rs`. Each page puts
//! its headline cards first, then its charts over the range picked in the
//! view's sub-tab row, then a table with one row per thing (plant, line,
//! good), then the figures as they stand now.

use bevy::prelude::Vec2;
use bevy_egui::egui::{self, Color32};
use nd_sim::{BusinessKind, LandUse, PowerPlantKind, Resource as Good, Tier};

use super::pane::{self, heading, row, Series, Span};
use super::tools::warn_if;
use super::{hint, zone_color32, Info, UiCtx, GOLD, IVORY, WARN};
use crate::tools::Selection;

const GOOD: Color32 = Color32::from_rgb(110, 200, 140);

// ── City ───────────────────────────────────────────────────────────────────

pub fn city_overview(ui: &mut egui::Ui, p: &mut UiCtx) {
    // [identity] The city's name, renamable.
    super::saves::city_name_row(ui, p.sim, p.flags);
    ui.add_space(4.0);
    let span = pane::data_span(ui.ctx());
    let w = &p.sim.world;
    let e = &w.economy;
    let h = &e.history;
    pane::cards(ui, |ui| {
        pane::kpi(ui, "people", egui::RichText::new(format!("{}", w.citizens.len())).color(GOLD));
        pane::kpi(ui, "homes filled", egui::RichText::new(format!("{}/{}", e.homes_total - e.homes_free, e.homes_total)));
        pane::kpi(ui, "jobs filled", egui::RichText::new(format!("{}/{}", e.jobs_total - e.jobs_free, e.jobs_total)));
        pane::kpi(ui, "buildings", egui::RichText::new(format!("{}", w.buildings.len())));
        pane::kpi(ui, "cars on the road", egui::RichText::new(format!("{}", w.traffic.cars.len())));
    });
    let days = pane::days(h, span);
    let labels = pane::day_labels(&days);
    heading(ui, "Homes and jobs");
    let series = [
        Series::new("homes", Color32::from_rgb(110, 200, 220), pane::col(&days, |d| d.homes as f32)),
        Series::new("homes filled", Color32::from_rgb(60, 130, 160), pane::col(&days, |d| d.homes_filled as f32)),
        Series::new("jobs", GOLD, pane::col(&days, |d| d.jobs as f32)),
        Series::new("jobs filled", Color32::from_rgb(170, 120, 50), pane::col(&days, |d| d.jobs_filled as f32)),
    ];
    pane::line_chart(ui, &labels, &series, 120.0, &|x| format!("{x:.0}"));
    legend(ui, &series);
    heading(ui, "Buildings");
    let series = [Series::new("standing", IVORY, pane::col(&days, |d| d.buildings as f32)), Series::new("building sites", GOLD, pane::col(&days, |d| d.sites as f32))];
    pane::line_chart(ui, &labels, &series, 90.0, &|x| format!("{x:.0}"));
    legend(ui, &series);
    heading(ui, "Traffic, last 72 hours");
    let hours = pane::hours(h);
    pane::line_chart(ui, &pane::hour_labels(&hours), &[Series::new("vehicles on the road", WARN, pane::hcol(&hours, |r| r.cars as f32))], 90.0, &|x| format!("{x:.0}"));
    city_figures(ui, p);
}

fn legend(ui: &mut egui::Ui, series: &[Series]) {
    pane::legend(ui, &series.iter().map(|s| (s.name.as_str(), s.color)).collect::<Vec<_>>());
}

// ── Market ─────────────────────────────────────────────────────────────────

/// Market, Goods and prices: every good against its base, and the stock.
pub fn market_goods(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let m = &w.economy.market;
    let scarce = Good::ALL.iter().filter(|r| m.avg_price.get(r).copied().unwrap_or(r.base_price()) > r.base_price() * 1.25).count();
    pane::cards(ui, |ui| {
        pane::kpi(ui, "goods traded", egui::RichText::new(format!("{}", Good::ALL.len())).color(GOLD));
        pane::kpi(ui, "scarce now", warn_if(scarce > 0, format!("{scarce}")));
        pane::kpi(ui, "deals today", egui::RichText::new(format!("{}", m.deals_today)));
        pane::kpi(ui, "meals sold today", egui::RichText::new(format!("{}", m.meals_sold_today)));
        pane::kpi(ui, "hungry", warn_if(m.hungry > 0, format!("{}", m.hungry)));
    });
    heading(ui, "Every good");
    hint(ui, "The average price paid in town against the good's base price, and the stock in town; red is scarce (over 125% of base).");
    pane::table(ui, "market_prices_all", &["good", "price", "base", "vs base", "stock"], |ui| {
        for r in Good::ALL {
            let price = m.avg_price.get(&r).copied().unwrap_or(r.base_price());
            let stock = m.stock.get(&r).copied().unwrap_or(0.0);
            let ratio = price / r.base_price().max(1e-3);
            ui.label(r.label());
            ui.label(warn_if(ratio > 1.25, format!("{price:.1}")));
            ui.label(egui::RichText::new(format!("{:.1}", r.base_price())).weak());
            ui.horizontal(|ui| {
                ui.label(warn_if(ratio > 1.25, format!("{:.0}%", ratio * 100.0)));
                ratio_bar(ui, ratio);
            });
            ui.label(egui::RichText::new(format!("{stock:.0}")).weak());
            ui.end_row();
        }
    });
}

/// A price against its base as a bar: the middle tick is the base, the bar
/// runs to the price (up to twice the base), red over 125%.
fn ratio_bar(ui: &mut egui::Ui, ratio: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(100.0, 9.0), egui::Sense::hover());
    let col = if ratio > 1.25 { WARN } else { GOOD };
    ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width() * (ratio / 2.0).clamp(0.0, 1.0), r.height())), 1.0, col.gamma_multiply(0.8));
    ui.painter().vline(r.center().x, r.y_range(), egui::Stroke::new(1.0, Color32::from_gray(150)));
}

/// Market, Businesses: how many trade, how each kind pays, and the demand
/// for new buildings by land use.
pub fn market_businesses(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::data_span(ui.ctx());
    let w = &p.sim.world;
    let e = &w.economy;
    let m = &e.market;
    let mut count = [0u32; BusinessKind::COUNT];
    let mut open = [0u32; BusinessKind::COUNT];
    for b in w.buildings.values() {
        if let Some(bz) = &b.business {
            count[bz.kind.index()] += 1;
            open[bz.kind.index()] += u32::from(!bz.closed);
        }
    }
    let losing = BusinessKind::ALL.iter().filter(|k| count[k.index()] > 0 && e.cover_of(**k).is_some_and(|c| c < 1.0)).count();
    pane::cards(ui, |ui| {
        pane::kpi(ui, "businesses", egui::RichText::new(format!("{}", m.businesses)).color(GOLD));
        pane::kpi(ui, "open now", egui::RichText::new(format!("{}", open.iter().sum::<u32>())));
        pane::kpi(ui, "closed so far", egui::RichText::new(format!("{}", m.closed_total)));
        pane::kpi(ui, "kinds losing money", warn_if(losing > 0, format!("{losing}")));
        pane::kpi(ui, "deals today", egui::RichText::new(format!("{}", m.deals_today)));
    });
    let days = pane::days(&e.history, span);
    let labels = pane::day_labels(&days);
    heading(ui, "Businesses and deals");
    let series = [Series::new("businesses", GOLD, pane::col(&days, |d| d.businesses as f32)), Series::new("deals that day", Color32::from_rgb(110, 200, 220), pane::col(&days, |d| d.deals as f32))];
    pane::line_chart(ui, &labels, &series, 120.0, &|x| format!("{x:.0}"));
    legend(ui, &series);
    heading(ui, "How each kind of business pays");
    hint(ui, "Takings over costs, smoothed over a few days; under 100% loses money. The bar's middle tick is 100%.");
    pane::table(ui, "business_cover", &["kind", "open", "takings / costs", ""], |ui| {
        for k in BusinessKind::ALL {
            let i = k.index();
            if count[i] == 0 && e.cover_of(k).is_none() {
                continue;
            }
            ui.label(k.label());
            ui.label(egui::RichText::new(format!("{} of {}", open[i], count[i])).weak());
            match e.cover_of(k) {
                Some(c) => {
                    ui.label(warn_if(c < 1.0, format!("{:.0}%", c * 100.0)));
                    let (r, _) = ui.allocate_exact_size(egui::vec2(140.0, 9.0), egui::Sense::hover());
                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width() * (c / 2.0).min(1.0), r.height())), 1.0, if c < 1.0 { WARN } else { GOOD });
                    ui.painter().vline(r.center().x, r.y_range(), egui::Stroke::new(1.0, Color32::from_gray(150)));
                }
                None => {
                    ui.label(egui::RichText::new("-").weak());
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });
    heading(ui, "Demand for new buildings");
    hint(ui, "How quickly vacant lots of each land use develop: now, and over the range.");
    for lu in LandUse::ALL {
        let col = zone_color32(p.assets.palette.zone_color(lu));
        ui.add(egui::ProgressBar::new(e.demand_for(lu)).text(lu.label()).fill(col.gamma_multiply(0.7)).desired_height(18.0).desired_width(BAR_WIDTH));
    }
    ui.add_space(6.0);
    let series: Vec<Series> = LandUse::ALL.iter().map(|&lu| Series::new(lu.label(), zone_color32(p.assets.palette.zone_color(lu)), pane::col(&days, |d| d.demand[lu.index()] * 100.0))).collect();
    pane::line_chart(ui, &labels, &series, 120.0, &|x| format!("{x:.0}%"));
    legend(ui, &series);
}

/// Market, Trade: what the city's businesses buy from and sell out of town.
pub fn market_trade(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::data_span(ui.ctx());
    let w = &p.sim.world;
    let e = &w.economy;
    let m = &e.market;
    let days = pane::days(&e.history, span);
    let n = days.len().max(1) as f32;
    pane::cards(ui, |ui| {
        pane::kpi(ui, "imports today", egui::RichText::new(format!("{:.0}", m.imports_today)).color(WARN));
        pane::kpi(ui, "exports today", egui::RichText::new(format!("{:.0}", m.exports_today)).color(GOOD));
        pane::kpi(ui, "building materials today", egui::RichText::new(format!("{:.0}", e.materials_today)));
        pane::kpi(ui, &format!("avg imports/day over {} d", days.len()), egui::RichText::new(format!("{:.0}", days.iter().map(|d| d.imports).sum::<f32>() / n)));
        pane::kpi(ui, &format!("avg exports/day over {} d", days.len()), egui::RichText::new(format!("{:.0}", days.iter().map(|d| d.exports).sum::<f32>() / n)));
    });
    let labels = pane::day_labels(&days);
    heading(ui, "Trade per day");
    let series = [Series::new("imports", WARN, pane::col(&days, |d| d.imports)), Series::new("exports", GOOD, pane::col(&days, |d| d.exports)), Series::new("building materials", Color32::from_gray(160), pane::col(&days, |d| d.materials))];
    pane::line_chart(ui, &labels, &series, 150.0, &|x| format!("{x:.0}"));
    legend(ui, &series);
    hint(ui, "The value of goods bought from and sold out of town by the city's businesses: their money, not the treasury's. The treasury's cut is the import duty (Economy, Taxes).");
    heading(ui, "Today");
    row(ui, "deals", format!("{}", m.deals_today));
    row(ui, "imports", format!("{:.0}", m.imports_today));
    row(ui, "exports", format!("{:.0}", m.exports_today));
    row(ui, "building materials", format!("{:.0}", e.materials_today));
    row(ui, "meals", warn_if(m.hungry > 0, format!("{} sold   {} hungry", m.meals_sold_today, m.hungry)));
    row(ui, "walk-in sales", format!("{:.0}", e.walk_in_sales_today));
}

/// How wide a progress bar gets on a page: wider reads as a chart.
pub const BAR_WIDTH: f32 = 440.0;

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
    egui::Grid::new(if wide { "power_plants_wide" } else { "power_plants" }).num_columns(cols).spacing(egui::vec2(16.0, 4.0)).striped(true).show(ui, |ui| {
        let heads: &[(&str, &str)] = if wide {
            &[("plant", ""), ("now", "output now / nameplate"), ("24 h mean", "mean output over the last 24 hours"), ("upkeep", "running cost per day"), ("cost/MWh", "upkeep over the last 24 hours' energy"), ("CO2 t/day", "carbon dioxide at the last 24 hours' rate"), ("smog", "air at the fence now, 0 to 1 (coal at full output is 1)"), ("noise", "at the fence while running"), ("", "")]
        } else {
            &[("plant", ""), ("now", "output now"), ("upkeep", "running cost per day"), ("cost/MWh", "upkeep over the last 24 hours' energy"), ("air", "CO2 per day; smog at the fence now"), ("noise", "at the fence while running")]
        };
        for (h, tip) in heads {
            let l = pane::th(ui, h);
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
            let name = ui.label(egui::RichText::new(&r.name).color(color));
            // [plant-groups] A Sunfield or Wind Park: its lots.
            let lots = if r.members.len() > 1 { format!(", {} lots, {:.0} m²", r.members.len(), r.area_m2) } else if r.area_m2 > 0.0 && matches!(r.asset, nd_sim::GridAsset::Plant(k) if k.reference_area().is_some()) { format!(", {:.0} m²", r.area_m2) } else { String::new() };
            name.on_hover_text(format!("{kind}{lots}{status}"));
            let cost = r.cost_per_mwh.map_or("-".to_string(), |c| format!("{c:.1}"));
            let noise = r.noise_db.map_or("quiet".to_string(), |d| format!("{d:.0} dB"));
            match r.asset {
                nd_sim::GridAsset::Plant(_) => {
                    let pct = if r.capacity_mw > 0.0 { r.now_mw / r.capacity_mw * 100.0 } else { 0.0 };
                    let smog = egui::RichText::new(format!("{:.2}", r.air));
                    ui.label(egui::RichText::new(if wide { format!("{:.1} MW · {pct:.0}% of {:.0}", r.now_mw, r.capacity_mw) } else { format!("{:.1} MW", r.now_mw) }));
                    if wide {
                        ui.label(egui::RichText::new(format!("{:.1} MW", r.mean_mw)).weak());
                    }
                    ui.label(egui::RichText::new(format!("{:.0}", r.upkeep_per_day)));
                    ui.label(egui::RichText::new(cost));
                    if wide {
                        ui.label(warn_if(r.co2_t_per_day > 100.0, format!("{:.0}", r.co2_t_per_day)));
                        ui.label(if r.air > 0.5 { smog.color(WARN) } else { smog });
                    } else {
                        ui.label(warn_if(r.co2_t_per_day > 100.0, if r.co2_t_per_day > 0.0 { format!("{:.0} t CO2 · smog {:.1}", r.co2_t_per_day, r.air) } else { "clean".to_string() }));
                    }
                    ui.label(warn_if(r.noise_db.is_some_and(|d| d >= 60.0), noise));
                }
                nd_sim::GridAsset::Battery(_) => {
                    let level = if r.capacity_mwh > 0.0 { r.charge_mwh / r.capacity_mwh } else { 0.0 };
                    let flow = if r.now_mw > 0.05 { format!("gives {:.1} MW", r.now_mw) } else if r.now_mw < -0.05 { format!("takes {:.1} MW", -r.now_mw) } else { "holding".to_string() };
                    ui.label(egui::RichText::new(if wide { format!("{:.0}% of {:.0} MWh · {flow}", level * 100.0, r.capacity_mwh) } else { format!("{:.0}% · {flow}", level * 100.0) }));
                    if wide {
                        ui.label(egui::RichText::new(format!("{:.0} MWh out", r.mean_mw * 24.0)).weak());
                    }
                    ui.label(egui::RichText::new(format!("{:.0}", r.upkeep_per_day)));
                    ui.label(egui::RichText::new(cost));
                    if wide {
                        ui.label(egui::RichText::new("0").weak());
                        ui.label(egui::RichText::new("0.00").weak());
                    } else {
                        ui.label(egui::RichText::new("clean").weak());
                    }
                    ui.label(egui::RichText::new(noise));
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
    let span = pane::data_span(ui.ctx());
    let act: Option<(nd_sim::BuildingId, Vec2)>;
    {
        let w = &p.sim.world;
        let pw = &w.economy.power;
        let h = &w.economy.history;
        pane::cards(ui, |ui| {
            pane::kpi(ui, "served", warn_if(pw.served < 0.98, format!("{:.0}%", pw.served * 100.0)).color(if pw.served < 0.98 { WARN } else { GOOD }));
            pane::kpi(ui, "supply now", egui::RichText::new(format!("{:.0} MW", pw.supply_mw)).color(GOLD));
            pane::kpi(ui, "demand now", egui::RichText::new(format!("{:.0} MW", pw.demand_mw)));
            pane::kpi(ui, "imported now", egui::RichText::new(format!("{:.0} MW", pw.imported_mw)).color(IMPORT));
            pane::kpi(ui, "exported now", egui::RichText::new(format!("{:.0} MW", pw.exported_mw)).color(EXPORT));
            if pw.batteries > 0 {
                let level = if pw.battery_capacity_mwh > 0.0 { pw.battery_charge_mwh / pw.battery_capacity_mwh } else { 0.0 };
                pane::kpi(ui, "batteries full", egui::RichText::new(format!("{:.0}%", level * 100.0)).color(BATTERY));
                // [batteries] The hour's rule and what they saved today.
                let rule = match pw.battery_mode {
                    nd_sim::power::BatteryMode::Charge => "charging (cheap)",
                    nd_sim::power::BatteryMode::Hold => "holding",
                    nd_sim::power::BatteryMode::Deliver => "delivering (dear)",
                };
                pane::kpi(ui, "battery rule", egui::RichText::new(rule).color(BATTERY));
                pane::kpi(ui, "saved today", egui::RichText::new(format!("{:.0}", pw.battery_savings_today)).color(if pw.battery_savings_today >= 0.0 { GOOD } else { WARN }));
            }
            pane::kpi(ui, "in the dark", warn_if(pw.unpowered_buildings > 0, format!("{}", pw.unpowered_buildings)));
        });
        let used: Vec<PowerPlantKind> = PowerPlantKind::ALL.into_iter().filter(|k| h.hours.iter().any(|r| r.supply_by_kind[k.index()] > 0.0) || h.days.iter().any(|d| d.supply_mwh[k.index()] > 0.0)).collect();
        heading(ui, "Supply and demand, last 72 hours");
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
        heading(ui, "Grid price and temperature, last 72 hours");
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
            heading(ui, "Batteries, last 72 hours");
            let series = [
                Series::new("stored MWh", BATTERY, pane::hcol(&hours, |r| r.battery_charge_mwh)),
                Series::new("capacity MWh", Color32::from_gray(120), pane::hcol(&hours, |r| r.battery_capacity_mwh)),
                Series::new("charging MW", CHARGING, pane::hcol(&hours, |r| r.battery_in_mw)),
                Series::new("delivering MW", Color32::from_rgb(255, 190, 80), pane::hcol(&hours, |r| r.battery_out_mw)),
            ];
            pane::line_chart(ui, &hour_labels, &series, 110.0, &|x| format!("{x:.1}"));
            legend(ui, &series);
            hint(ui, "How full the batteries were at the end of each hour, and when they took surplus in or carried the city (about 90% of what goes in comes back out).");
            let b = pw.price_bands;
            hint(ui, &format!("The rule: a surplus always goes into the batteries. In the day's six cheapest hours (power at {:.2}/MWh or less) they also buy from the regional grid to fill up; in its six dearest (from {:.2}) they deliver before anything is imported; in between they hold the charge for the peak, giving only what the import line cannot. Now: {}.", b.cheap, b.dear, pw.battery_mode.label()));
        }

        let days = pane::days(h, if span == Span::Hours { Span::Week } else { span });
        heading(ui, "Energy per day");
        let mut series: Vec<Series> = used.iter().map(|k| Series::new(k.short(), plant_color(*k), pane::col(&days, |d| d.supply_mwh[k.index()]))).collect();
        series.push(Series::new("batteries delivering", BATTERY, pane::col(&days, |d| d.battery_out_mwh)));
        series.push(Series::new("imported", IMPORT, pane::col(&days, |d| d.imported_mwh)));
        series.push(Series::new("into batteries", CHARGING, pane::col(&days, |d| -d.battery_in_mwh)));
        series.push(Series::new("exported", EXPORT, pane::col(&days, |d| -d.exported_mwh)));
        series.retain(|s| s.values.iter().any(|v| *v != 0.0) || used.iter().any(|k| k.short() == s.name));
        let demand = Series::new("demand", DEMAND, pane::col(&days, |d| d.demand_mwh));
        pane::stacked_bars(ui, &pane::day_labels(&days), &series, 130.0, false, Some(&demand), &|x| format!("{x:.0} MWh"));
        let mut lg: Vec<(&str, Color32)> = series.iter().map(|s| (s.name.as_str(), s.color)).collect();
        lg.push(("demand (line)", DEMAND));
        pane::legend(ui, &lg);
        heading(ui, "Money for power per day");
        let series = [
            Series::new("export income", EXPORT, pane::col(&days, |d| d.flow(nd_sim::economy::Flow::GridExport))),
            Series::new("import cost", IMPORT, pane::col(&days, |d| d.flow(nd_sim::economy::Flow::GridImport))),
            Series::new("upkeep", Color32::from_rgb(255, 120, 90), pane::col(&days, |d| d.flow(nd_sim::economy::Flow::PlantUpkeep))),
        ];
        pane::stacked_bars(ui, &pane::day_labels(&days), &series, 100.0, true, None, &|x| format!("{x:.0}"));
        legend(ui, &series);
        // [batteries] What the batteries saved each day: imports spared at
        // the hour's price, less power bought to charge them and exports
        // forgone.
        if days.iter().any(|d| d.battery_savings != 0.0) || pw.batteries > 0 {
            heading(ui, "Battery savings per day");
            let series = [Series::new("saved", BATTERY, pane::col(&days, |d| d.battery_savings))];
            pane::stacked_bars(ui, &pane::day_labels(&days), &series, 70.0, true, None, &|x| format!("{x:.0}"));
            hint(ui, &format!("Imports the batteries spared (at that hour's price) less the power bought to charge them and the exports their surplus would have earned. Today so far: {:.0}.", pw.battery_savings_today));
        }
        heading(ui, "Brownouts and peak");
        let series = [Series::new("brownout hours", WARN, pane::col(&days, |d| d.brownout_hours as f32)), Series::new("peak demand MW", IVORY, pane::col(&days, |d| d.peak_demand_mw))];
        pane::line_chart(ui, &pane::day_labels(&days), &series, 90.0, &|x| format!("{x:.0}"));
        legend(ui, &series);

        heading(ui, "Plants and batteries");
        let rows = w.plant_rows();
        if rows.is_empty() {
            hint(ui, "No plants: the city buys what the regional grid will sell. Buildings, Power places plants and battery packs.");
        }
        act = plant_table(ui, w, &rows, true);
        if !rows.is_empty() {
            heading(ui, "Each plant, last 72 hours");
            let n = hours.len();
            for r in &rows {
                let Some(log) = h.plant(r.id) else { continue };
                if n == 0 {
                    break;
                }
                match r.asset {
                    nd_sim::GridAsset::Plant(k) => {
                        // [plant-groups] A park's members summed hour by hour.
                        let what = if r.members.len() > 1 { format!("{} lots of {}", r.members.len(), k.label()) } else { k.label().to_string() };
                        ui.label(egui::RichText::new(format!("{} · {what}", r.name)).small().color(plant_color(k)));
                        let summed = w.plant_row_hours(r).unwrap_or_default();
                        let s = [Series::new("output MW", plant_color(k), plant_hours(&summed, n)), Series::new("nameplate", Color32::from_gray(90), vec![r.capacity_mw; n])];
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
    power_figures(ui, p);
}

// ── Transit ────────────────────────────────────────────────────────────────

pub fn transit(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::data_span(ui.ctx());
    let mut pick: Option<Option<nd_sim::RouteId>> = None;
    {
        let w = &p.sim.world;
        let e = &w.economy;
        let h = &e.history;
        let l = &e.ledger;
        use nd_sim::economy::Flow;
        let fares = l.yesterday_of(Flow::BusFares) + l.yesterday_of(Flow::RailFares);
        let upkeep = l.yesterday_of(Flow::TransitUpkeep) + l.yesterday_of(Flow::RailUpkeep);
        pane::cards(ui, |ui| {
            pane::kpi(ui, "bus lines", egui::RichText::new(format!("{}", w.transit.routes.len())).color(GOLD));
            pane::kpi(ui, "buses out", egui::RichText::new(format!("{}", e.transit.buses)));
            pane::kpi(ui, "riders today", egui::RichText::new(format!("{}", e.transit.riders_today + w.rail.stats.riders_today)));
            pane::kpi(ui, "fares yesterday", egui::RichText::new(format!("{fares:.0}")).color(GOOD));
            pane::kpi(ui, "upkeep yesterday", egui::RichText::new(format!("{upkeep:.0}")).color(WARN));
        });
        let days = pane::days(h, if span == Span::Hours { Span::Week } else { span });
        let labels = pane::day_labels(&days);
        heading(ui, "Riders per day by mode");
        let series = [
            Series::new("bus", GOLD, pane::col(&days, |d| d.bus_riders as f32)),
            Series::new("rail", Color32::from_rgb(150, 200, 255), pane::col(&days, |d| d.rail_riders as f32)),
            Series::new("ferry", Color32::from_rgb(70, 200, 190), pane::col(&days, |d| d.ferry_riders as f32)),
            Series::new("air taxi", Color32::from_rgb(200, 120, 230), pane::col(&days, |d| d.air_riders as f32)),
        ];
        pane::stacked_bars(ui, &labels, &series, 120.0, false, None, &|x| format!("{x:.0}"));
        legend(ui, &series);

        heading(ui, "Riders per day by bus line");
        let mut lines: Vec<(nd_sim::RouteId, String, f32)> = w.transit.routes.iter().map(|(id, r)| (id, r.name.clone(), days.iter().map(|d| d.riders_on(id) as f32).sum())).collect();
        lines.sort_by(|a, b| b.2.total_cmp(&a.2));
        let series: Vec<Series> = lines.iter().take(10).enumerate().map(|(i, (id, name, _))| Series::new(name.clone(), pane::palette(i), pane::col(&days, |d| d.riders_on(*id) as f32))).collect();
        if series.is_empty() {
            hint(ui, "No bus lines yet: the Buses tool draws them.");
        } else {
            pane::line_chart(ui, &labels, &series, 120.0, &|x| format!("{x:.0}"));
            legend(ui, &series);
        }

        heading(ui, "Fares against upkeep per day");
        let series: Vec<Series> = [Flow::BusFares, Flow::RailFares, Flow::TransitUpkeep, Flow::RailUpkeep].iter().map(|f| Series::new(f.label(), super::economy::flow_color(*f), pane::col(&days, |d| d.flow(*f)))).collect();
        pane::stacked_bars(ui, &labels, &series, 110.0, true, None, &|x| format!("{x:+.0}"));
        legend(ui, &series);

        heading(ui, "On the move, last 72 hours");
        let hours = pane::hours(h);
        let series = [Series::new("waiting for a bus", WARN, pane::hcol(&hours, |r| r.bus_waiting as f32)), Series::new("on a bus", GOLD, pane::hcol(&hours, |r| r.bus_riding as f32)), Series::new("on a train", Color32::from_rgb(150, 200, 255), pane::hcol(&hours, |r| r.rail_riding as f32))];
        pane::line_chart(ui, &pane::hour_labels(&hours), &series, 100.0, &|x| format!("{x:.0}"));
        legend(ui, &series);

        if !w.transit.routes.is_empty() {
            heading(ui, "Bus lines");
            hint(ui, "Click a line to pick it: it shows on the map when the view closes. The Lines window (top bar) edits it stop by stop.");
            let fares_of = |id: nd_sim::RouteId| l.items_yesterday.iter().filter(|e| e.item == nd_sim::economy::Item::Route(id)).map(|e| e.amount).sum::<f32>();
            pane::table(ui, "transit_lines", &["line", "stops", "buses", "riders today", "yesterday", "avg/day", "fares yesterday"], |ui| {
                for (id, r) in &w.transit.routes {
                    let sel = p.tools.route_sel == Some(id);
                    if ui.selectable_label(sel, egui::RichText::new(&r.name)).clicked() {
                        pick = Some(if sel { None } else { Some(id) });
                    }
                    ui.label(egui::RichText::new(format!("{}", r.stops.len())));
                    ui.label(egui::RichText::new(format!("{}", r.buses)));
                    ui.label(egui::RichText::new(format!("{}", r.riders_today)));
                    ui.label(egui::RichText::new(format!("{}", days.last().map_or(0, |d| d.riders_on(id)))));
                    ui.label(egui::RichText::new(format!("{:.0}", days.iter().map(|d| d.riders_on(id) as f32).sum::<f32>() / days.len().max(1) as f32)));
                    ui.label(egui::RichText::new(format!("{:.0}", fares_of(id))));
                    ui.end_row();
                }
            });
        }
    }
    if let Some(sel) = pick {
        p.tools.route_sel = sel;
    }
    transit_figures(ui, p);
}

// ── Land ───────────────────────────────────────────────────────────────────

pub fn city_land(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::data_span(ui.ctx());
    {
        let w = &p.sim.world;
        let e = &w.economy;
        let vacant = w.lots.values().filter(|l| l.building.is_none() && l.usable() && !w.is_city_ground(l)).count();
        pane::cards(ui, |ui| {
            pane::kpi(ui, "average land value", egui::RichText::new(format!("{:.2}", e.avg_land_value)).color(GOLD));
            for (i, t) in Tier::ALL.iter().enumerate() {
                pane::kpi(ui, &format!("{} rent/home/day", t.label()), egui::RichText::new(format!("{:.0}", e.avg_rent[i])).color(super::people::tier_color(*t)));
            }
            pane::kpi(ui, "vacant lots", egui::RichText::new(format!("{vacant}")));
        });
        let days = pane::days(&w.economy.history, span);
        let labels = pane::day_labels(&days);
        heading(ui, "Land value");
        pane::line_chart(ui, &labels, &[Series::new("average land value", GOLD, pane::col(&days, |d| d.avg_land_value))], 90.0, &|x| format!("{x:.2}"));
        heading(ui, "Rent by housing tier, per day");
        let series: Vec<Series> = Tier::ALL.iter().enumerate().map(|(i, t)| Series::new(t.label(), super::people::tier_color(*t), pane::col(&days, |d| d.avg_rent[i]))).collect();
        pane::line_chart(ui, &labels, &series, 100.0, &|x| format!("{x:.0}"));
        legend(ui, &series);
        heading(ui, "Lots by land use");
        pane::table(ui, "land_lots", &["land use", "lots", "built", "vacant"], |ui| {
            for lu in LandUse::ALL {
                // [free-placement] City ground is out of the zones: its own row below.
                let lots: Vec<&nd_sim::Lot> = w.lots.values().filter(|l| l.land_use == lu && !w.is_city_ground(l)).collect();
                if lots.is_empty() {
                    continue;
                }
                let built = lots.iter().filter(|l| l.building.is_some()).count();
                ui.label(egui::RichText::new(lu.label()).color(zone_color32(p.assets.palette.zone_color(lu))));
                ui.label(egui::RichText::new(format!("{}", lots.len())));
                ui.label(egui::RichText::new(format!("{built}")));
                ui.label(egui::RichText::new(format!("{}", lots.iter().filter(|l| l.building.is_none() && l.usable()).count())));
                ui.end_row();
            }
            let city = w.lots.values().filter(|l| w.is_city_ground(l)).count();
            if city > 0 {
                ui.label(egui::RichText::new("City buildings").color(zone_color32(p.assets.palette.zone_color(LandUse::Civic))));
                ui.label(egui::RichText::new(format!("{city}")));
                ui.label(egui::RichText::new(format!("{city}")));
                ui.label(egui::RichText::new("0"));
                ui.end_row();
            }
        });
    }
    land_figures(ui, p);
}

/// Land value and who lives where: the figures the View window had.
fn land_figures(ui: &mut egui::Ui, p: &mut UiCtx) {
    let e = &p.sim.world.economy;
    heading(ui, "Now");
    row(ui, "average land value", format!("{:.2}", e.avg_land_value));
    row(ui, "class mix", format!("low {} · middle {} · high {}", e.class_counts[0], e.class_counts[1], e.class_counts[2]));
    row(ui, "housing by tier", format!("low {} · middle {} · high {}", e.tier_counts[0], e.tier_counts[1], e.tier_counts[2]));
    row(ui, "rent per home per day", format!("low {:.0} · middle {:.0} · high {:.0}", e.avg_rent[0], e.avg_rent[1], e.avg_rent[2]));
    hint(ui, "The Land value layer (V, or the top bar's Layer button) shows it on the map.");
}

// ── figures (what the compact windows showed) ─────────────────────────────

/// The city's standing figures, under the charts on City, Overview.
pub fn city_figures(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let e = &w.economy;
    let sites = w.buildings.values().filter(|b| b.construction.is_some()).count();
    heading(ui, "Buildings and lots now");
    row(ui, "buildings", format!("{}   under construction {}", w.buildings.len() - sites, sites));
    row(ui, "lots", format!("{} ({} vacant, {} unusable)", w.lots.len(), w.lots.values().filter(|l| l.building.is_none() && l.usable()).count(), w.lots.values().filter(|l| l.landlocked && l.park.is_none() && l.land_use != LandUse::Park).count()));
    if sites > 0 {
        row(ui, "sites will add", format!("{} homes and {} jobs", e.homes_building, e.jobs_building));
    }
    heading(ui, "Homes and jobs now");
    row(ui, "homes", format!("{}/{}", e.homes_total - e.homes_free, e.homes_total));
    row(ui, "jobs", format!("{}/{}", e.jobs_total - e.jobs_free, e.jobs_total));
    row(ui, "waiting to move in", format!("{}", e.waiting_immigrants));
    heading(ui, "Streets now");
    row(ui, "cars on the road", format!("{}   lanes {}   signalled junctions {}", w.traffic.cars.len(), w.traffic.net.lanes.len(), w.traffic.net.intersections.iter().filter(|i| i.signalized).count()));
    row(ui, "parking", format!("{}/{} spaces in {} lots   circling today {}   fees today {:.0}", e.parking_used, e.parking_capacity, e.parking_lots, e.parking_street_today, e.parking_fees_today));
    row(ui, "walking", format!("{} trips today   walk-in sales {:.0}   {} pedestrian nodes", e.walk_trips_today, e.walk_in_sales_today, w.walk.nodes.len()));
}

/// The grid now, the batteries, the plants, costs, the next day and the weather.
pub fn power_figures(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let pw = &w.economy.power;
    heading(ui, "Grid");
    ui.add(egui::ProgressBar::new(pw.served).text(format!("served {:.0}%", pw.served * 100.0)).fill(if pw.served < 0.98 { WARN.gamma_multiply(0.8) } else { egui::Color32::from_rgb(110, 200, 140).gamma_multiply(0.8) }).desired_height(18.0).desired_width(BAR_WIDTH));
    row(ui, "supply", format!("{:.0} MW from {} plant{}", pw.supply_mw, pw.plants, if pw.plants == 1 { "" } else { "s" }));
    row(ui, "demand", format!("{:.0} MW", pw.demand_mw));
    // Where the balance went this hour: the merit order of `power::dispatch`.
    let mut balance = Vec::new();
    if pw.battery_out_mw > 0.0 {
        balance.push(format!("batteries give {:.0} MW", pw.battery_out_mw));
    }
    if pw.battery_in_mw > 0.0 {
        balance.push(format!("batteries take {:.0} MW", pw.battery_in_mw));
    }
    if pw.imported_mw > 0.0 {
        balance.push(format!("imported {:.0} MW", pw.imported_mw));
    }
    if pw.exported_mw > 0.0 {
        balance.push(format!("exported {:.0} MW", pw.exported_mw));
    }
    if pw.curtailed_mw > 0.5 {
        balance.push(format!("{:.0} MW turned down", pw.curtailed_mw));
    }
    if !balance.is_empty() {
        row(ui, "balance", balance.join("   "));
    }
    row(ui, "in the dark", warn_if(pw.unpowered_buildings > 0, format!("{} buildings   {} brownout hours today", pw.unpowered_buildings, pw.brownout_hours_today)));
    if pw.batteries > 0 {
        heading(ui, "Batteries");
        let level = if pw.battery_capacity_mwh > 0.0 { pw.battery_charge_mwh / pw.battery_capacity_mwh } else { 0.0 };
        let state = if pw.battery_out_mw > 0.0 { format!("delivering {:.0} MW", pw.battery_out_mw) } else if pw.battery_in_mw > 0.0 { format!("charging {:.0} MW", pw.battery_in_mw) } else { "holding".to_string() };
        ui.add(egui::ProgressBar::new(level).text(format!("{:.0} of {:.0} MWh · {state}", pw.battery_charge_mwh, pw.battery_capacity_mwh)).fill(BATTERY.gamma_multiply(0.8)).desired_height(18.0).desired_width(BAR_WIDTH));
    }
    heading(ui, "Costs");
    row(ui, "upkeep", format!("{:.0}/day (plants and batteries)", pw.upkeep_today));
    row(ui, "grid import", format!("{:.0} today (up to {:.0} MW, now {:.2}/MWh)", pw.import_cost_today, nd_sim::power::GRID_IMPORT_MW, pw.import_price));
    row(ui, "grid export", format!("+{:.0} today, +{:.0} yesterday (up to {:.0} MW, now {:.2}/MWh)", pw.export_income_today, w.economy.ledger.yesterday_of(nd_sim::economy::Flow::GridExport), nd_sim::power::GRID_EXPORT_MW, pw.export_price));
    // [grid-weather] Time-of-use: the next day's demand and price.
    heading(ui, "Next 24 hours");
    let ahead = w.grid_day_ahead();
    let price_word = if pw.import_price < nd_sim::power::GRID_IMPORT_PRICE * 0.7 { "cheap" } else if pw.import_price > nd_sim::power::GRID_IMPORT_PRICE * 1.3 { "peak" } else { "middling" };
    let (dear, cheap) = ahead.iter().fold((ahead[0], ahead[0]), |(d, c), g| (if g.import_price > d.import_price { *g } else { d }, if g.import_price < c.import_price { *g } else { c }));
    row(ui, "price now", format!("{price_word}: buy {:.2}, sell {:.2} per MWh   dearest {:.2} at {:02}:00, cheapest {:.2} at {:02}:00", pw.import_price, pw.export_price, dear.import_price, dear.hour, cheap.import_price, cheap.hour));
    let peak = ahead.iter().map(|g| g.demand_mw).fold(0.0f32, f32::max);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("demand").small().weak());
        pane::sparkline(ui, &ahead.iter().map(|g| g.demand_mw).collect::<Vec<_>>(), egui::Color32::from_rgb(255, 90, 90), 26.0, &format!("The city's demand hour by hour from now, if the weather holds: peak {peak:.0} MW. Homes peak in the evening, offices in the day; heat and cold add air conditioning and heating."));
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("price  ").small().weak());
        pane::sparkline(ui, &ahead.iter().map(|g| g.import_price).collect::<Vec<_>>(), egui::Color32::from_rgb(255, 175, 80), 26.0, "The regional grid's import price hour by hour from now: cheap at night, dear in the evening peak and in heat waves. Batteries carry cheap or free power into the dear hours.");
    });
    heading(ui, "Weather");
    let x = &w.weather;
    let spell = if x.spell == nd_sim::power::Spell::Normal { String::new() } else { format!("   {} for {} more hours", x.spell.label(), x.spell_hours) };
    row(ui, x.label(), format!("{:.0} °C   cloud {:.0}%   wind {:.1} m/s{spell}", x.temp_c, x.cloud * 100.0, x.wind_ms()));
    hint(ui, &format!("{} climate. Solar dims under cloud and in great heat, and stops at night; wind farms follow the wind at their site and brake above {:.0} m/s in a storm. Heat waves and cold snaps raise demand (air conditioning, heating). Surplus charges the batteries, then is sold; a shortfall drains them, then is bought.", x.climate.label(), nd_sim::weather::CUT_OUT_MS));
}

/// Buses, rail, metro, taxis and cargo hubs now.
pub fn transit_figures(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let t = &w.economy.transit;
    heading(ui, "Buses");
    if t.depots == 0 {
        ui.label(egui::RichText::new("No depot: buses cannot run. The Buses tool places one.").small().color(WARN));
    }
    row(ui, "depots", format!("{} for {} buses   {} out on the lines", t.depots, t.depot_capacity, t.buses));
    row(ui, "riders", format!("{} waiting   {} riding   {} today", t.waiting, t.riding, t.riders_today));
    row(ui, "money", format!("fares {:.0} today   upkeep {:.0}/day", t.fares_today, t.upkeep_today));
    if ui.small_button("Lines: stops, times and ridership…").on_hover_text("Open the Lines window to tune a line stop by stop").clicked() {
        Info::Lines.set_open(ui.ctx(), true);
    }
    let r = &w.rail.stats;
    heading(ui, "Rail");
    row(ui, "network", format!("{} station{}   {} yard{}   {} train{} out", r.stations, if r.stations == 1 { "" } else { "s" }, r.yards, if r.yards == 1 { "" } else { "s" }, r.trains, if r.trains == 1 { "" } else { "s" }));
    row(ui, "passengers", format!("{} waiting   {} riding   {} today   next train in {} min", r.waiting, r.riding, r.riders_today, r.next_passenger_in));
    row(ui, "trains today", format!("{} passenger   {} freight", r.passenger_trains_today, r.freight_trains_today));
    row(ui, "freight", format!("{:.0} units in today   {:.0} waiting at yards", r.freight_in_today, r.freight_waiting + 0.0));
    row(ui, "money", format!("fares {:.0} today   upkeep {:.0}/day", r.fares_today, r.upkeep_today + 0.0));
    // [subway]
    let s = &w.subway.stats;
    heading(ui, "Metro");
    if s.lines == 0 {
        hint(ui, "No metro line yet: place metro stations (Build window, Transit, Metro), then Metro lines joins them station by station.");
    }
    row(ui, "network", format!("{} station{}   {} line{}   {:.1} km   {} train{} running", s.stations, if s.stations == 1 { "" } else { "s" }, s.lines, if s.lines == 1 { "" } else { "s" }, s.tunnel_m / 1000.0 + 0.0, s.trains, if s.trains == 1 { "" } else { "s" }));
    row(ui, "riders", format!("{} waiting   {} riding   {} today   {} yesterday", s.waiting, s.riding, s.riders_today, s.riders_yesterday));
    row(ui, "money", format!("fares {:.0} today ({:.0} yesterday)   upkeep {:.0} yesterday", s.fares_today, s.fares_yesterday, s.upkeep_today));
    for l in w.subway.lines.values() {
        row(ui, &l.name, format!("{} stations   every {} min   {} riders today   {} yesterday", l.stations.len(), l.headway, l.riders_today, l.riders_yesterday));
    }
    let mut busiest: Vec<(nd_sim::BuildingId, u32, u32)> = w.subway.boardings.clone();
    busiest.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
    for (b, today, yesterday) in busiest.into_iter().take(5) {
        if let Some(bld) = w.buildings.get(b) {
            row(ui, &bld.name, format!("{today} boarded today   {yesterday} yesterday"));
        }
    }
    taxis_and_freight(ui, w);
}

/// [taxis] Services, Transit: the taxi and cargo hub sections.
fn taxis_and_freight(ui: &mut egui::Ui, w: &nd_sim::World) {
    let t = &w.taxis;
    let s = &t.stats;
    let k = t.counts();
    heading(ui, "Taxis");
    let companies = w.taxi_companies();
    if companies.is_empty() {
        hint(ui, &format!("No taxi company yet. One opens when people want cabs: {} of {} trips today would have taken one.", t.latent_today, nd_sim::taxi::FOUND_DEMAND));
    } else {
        row(ui, "fleet", format!("{} cab{} · {} on duty · {} on the road · {} at ranks · {} in the yard", k.cabs, if k.cabs == 1 { "" } else { "s" }, k.on_duty, k.on_road, k.at_ranks + k.at_kerb, k.in_yard));
        row(ui, "rides", format!("{} today ({} yesterday) · {} carrying now · {} waiting at the kerb", s.rides_today, s.rides_yesterday, k.carrying, t.requests.len()));
        row(ui, "riders", format!("{} drunk · {} late at night · {} gave up waiting", s.drunk_rides_today, s.night_rides_today, s.gave_up_today));
        let wait = if s.rides_today > 0 { s.wait_minutes_today / s.rides_today as f32 } else { 0.0 };
        row(ui, "money", format!("fares {:.0} today · mean wait {:.0} min", s.fares_today, wait));
        for c in companies {
            let name = w.buildings.get(c).map(|b| b.name.as_str()).unwrap_or("?");
            let (rides, _) = t.rides_by_company.get(&c).copied().unwrap_or((0, 0));
            hint(ui, &format!("{name}: {} cabs, {rides} rides today", t.fleet_size(c)));
        }
        if t.latent_today > 0 {
            hint(ui, &format!("{} trips today wanted a cab out of reach of any company", t.latent_today));
        }
    }
    let h = &w.cargo_hubs;
    if !h.hubs.is_empty() {
        let hs = &h.stats;
        heading(ui, "Cargo hubs");
        let (mut waiting, mut inbound, mut shed) = (0.0f32, 0.0f32, 0.0f32);
        for st in h.hubs.values() {
            let [a, b, c, d] = st.units();
            waiting += a + b;
            inbound += c;
            shed += d;
        }
        row(ui, "hubs", format!("{} · {:.0} units waiting for a semi · {:.0} coming by rail or sea · {:.0} in the sheds", h.hubs.len(), waiting, inbound, shed));
        row(ui, "today", format!("{} loads in · {} sent on · {} semis in, {} out", hs.received_today, hs.forwarded_today, hs.trunks_in_today, hs.trunks_out_today));
        row(ui, "long haul", format!("{} trips through the city saved today ({} yesterday)", hs.long_haul_saved_today, hs.long_haul_saved_yesterday));
    }
}

