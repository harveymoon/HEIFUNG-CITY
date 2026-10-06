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
const DEMAND: Color32 = Color32::from_rgb(255, 90, 90);

pub fn power(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::span_picker(ui, Info::Power);
    let mut act: Option<(nd_sim::BuildingId, Vec2)> = None;
    {
        let w = &p.sim.world;
        let pw = &w.economy.power;
        let h = &w.economy.history;
        ui.horizontal_wrapped(|ui| {
            pane::kpi(ui, "served", warn_if(pw.served < 0.98, format!("{:.0}%", pw.served * 100.0)).color(if pw.served < 0.98 { WARN } else { GOOD }));
            pane::kpi(ui, "supply now", egui::RichText::new(format!("{:.0} MW", pw.supply_mw)).color(GOLD));
            pane::kpi(ui, "demand now", egui::RichText::new(format!("{:.0} MW", pw.demand_mw)));
            pane::kpi(ui, "imported now", egui::RichText::new(format!("{:.0} MW", pw.imported_mw)).color(IMPORT));
            pane::kpi(ui, "in the dark", warn_if(pw.unpowered_buildings > 0, format!("{}", pw.unpowered_buildings)));
        });
        let used: Vec<PowerPlantKind> = PowerPlantKind::ALL.into_iter().filter(|k| h.hours.iter().any(|r| r.supply_by_kind[k.index()] > 0.0) || h.days.iter().any(|d| d.supply_mwh[k.index()] > 0.0)).collect();
        section(ui, "SUPPLY AND DEMAND, LAST 72 HOURS");
        let hours = pane::hours(h);
        let mut series: Vec<Series> = used.iter().map(|k| Series::new(k.short(), plant_color(*k), pane::hcol(&hours, |r| r.supply_by_kind[k.index()]))).collect();
        series.push(Series::new("imported", IMPORT, pane::hcol(&hours, |r| r.imported_mw)));
        let demand = Series::new("demand", DEMAND, pane::hcol(&hours, |r| r.demand_mw));
        pane::stacked_bars(ui, &pane::hour_labels(&hours), &series, 140.0, false, Some(&demand), &|x| format!("{x:.1} MW"));
        let mut lg: Vec<(&str, Color32)> = series.iter().map(|s| (s.name.as_str(), s.color)).collect();
        lg.push(("demand (line)", DEMAND));
        pane::legend(ui, &lg);
        hint(ui, "Each hour's supply stacked by kind of plant, then what the regional grid sold; the red line is what the city wanted.");

        let days = pane::days(h, if span == Span::Hours { Span::Week } else { span });
        section(ui, "ENERGY PER DAY");
        let mut series: Vec<Series> = used.iter().map(|k| Series::new(k.short(), plant_color(*k), pane::col(&days, |d| d.supply_mwh[k.index()]))).collect();
        series.push(Series::new("imported", IMPORT, pane::col(&days, |d| d.imported_mwh)));
        let demand = Series::new("demand", DEMAND, pane::col(&days, |d| d.demand_mwh));
        pane::stacked_bars(ui, &pane::day_labels(&days), &series, 130.0, false, Some(&demand), &|x| format!("{x:.0} MWh"));
        section(ui, "BROWNOUTS AND PEAK");
        let series = [Series::new("brownout hours", WARN, pane::col(&days, |d| d.brownout_hours as f32)), Series::new("peak demand MW", IVORY, pane::col(&days, |d| d.peak_demand_mw))];
        pane::line_chart(ui, &pane::day_labels(&days), &series, 90.0, &|x| format!("{x:.0}"));
        legend(ui, &series);

        section(ui, "PLANTS");
        let mut plants: Vec<(nd_sim::BuildingId, &nd_sim::Building, PowerPlantKind)> = w.buildings.iter().filter_map(|(id, b)| b.plant.map(|k| (id, b, k))).collect();
        plants.sort_by(|a, b| a.2.index().cmp(&b.2.index()).then(a.1.name.cmp(&b.1.name)));
        if plants.is_empty() {
            hint(ui, "No plants: the city buys what the regional grid will sell. The Buildings tool places plants.");
        }
        egui::Grid::new("power_plants").num_columns(6).spacing(egui::vec2(12.0, 2.0)).striped(true).show(ui, |ui| {
            for hd in ["plant", "kind", "now", "of nameplate", "upkeep/day", ""] {
                ui.label(egui::RichText::new(hd).small().weak());
            }
            ui.end_row();
            for (id, b, k) in plants {
                let now = w.plant_output_mw(b);
                ui.label(egui::RichText::new(&b.name).small().color(plant_color(k)));
                ui.label(egui::RichText::new(if b.is_derelict() { format!("{} (derelict)", k.label()) } else { k.label().to_string() }).small().weak());
                ui.label(egui::RichText::new(format!("{now:.1} MW")).small());
                ui.label(egui::RichText::new(format!("{:.0}% of {:.0} MW", now / k.capacity_mw() * 100.0, k.capacity_mw())).small().weak());
                ui.label(egui::RichText::new(format!("{:.0}", k.upkeep_per_day())).small());
                if ui.small_button("go").on_hover_text("Select it and fly there").clicked() {
                    if let Some(l) = w.lots.get(b.lot) {
                        act = Some((id, Vec2::new(l.center.x, l.center.y)));
                    }
                }
                ui.end_row();
            }
        });
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
                let lots: Vec<&nd_sim::Lot> = w.lots.values().filter(|l| l.land_use == lu).collect();
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
        });
    }
    ui.separator();
    super::windows::land(ui, p);
}
