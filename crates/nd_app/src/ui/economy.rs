//! The Economy window. Compact: the treasury and yesterday's ledger as
//! signed bars. Expanded: where the money has been going over time and
//! what every ledger line is made of.
//!
//! The expanded view is a column of independent sections, each a function
//! taking the world and the chosen range (`expanded` lists them in order):
//! the headline figures, the treasury over time, income and spending by
//! flow over time, the ledger drill-down (every flow broken into the
//! buildings, lines, classes or land uses that made it, from the sim's
//! itemised ledger), the tax take by class and land use, and trade. A
//! taxes section (rates per sub-type, from the feat/taxes work) slots in
//! as one more function after `tax_take` without touching the others.

use bevy::prelude::Vec2;
use bevy_egui::egui::{self, Color32};
use nd_sim::economy::{Flow, Item, ItemEntry, Ledger};
use nd_sim::history::DayRecord;
use nd_sim::{Building, LandUse, Tier, World};

use super::pane::{self, Series, Span};
use super::tools::warn_if;
use super::{hint, section, zone_color32, Info, UiCtx, GOLD, WARN};
use crate::tools::Selection;

const INCOME: Color32 = Color32::from_rgb(110, 200, 140);

/// A fixed colour per flow: greens and blues for income, reds, oranges and
/// purples for spending, so the stacked chart reads by temperature.
pub fn flow_color(f: Flow) -> Color32 {
    match f {
        Flow::IncomeTax => Color32::from_rgb(110, 200, 140),
        Flow::BusinessTax => Color32::from_rgb(70, 170, 210),
        Flow::BusFares => Color32::from_rgb(120, 230, 200),
        Flow::RailFares => Color32::from_rgb(150, 200, 255),
        Flow::Tickets => Color32::from_rgb(230, 210, 110),
        Flow::ParkingFines => Color32::from_rgb(200, 170, 250),
        Flow::ParkingFees => Color32::from_rgb(170, 230, 120),
        Flow::PlantUpkeep => Color32::from_rgb(255, 120, 90),
        Flow::GridImport => Color32::from_rgb(255, 175, 80),
        Flow::TransitUpkeep => Color32::from_rgb(220, 90, 150),
        Flow::RailUpkeep => Color32::from_rgb(170, 100, 210),
        Flow::Construction => Color32::from_rgb(150, 150, 165),
        Flow::ServiceUpkeep => Color32::from_rgb(235, 70, 80),
        Flow::TouristTax => Color32::from_rgb(255, 140, 220),
        Flow::SubwayFares => Color32::from_rgb(90, 210, 255),
        Flow::SubwayUpkeep => Color32::from_rgb(200, 60, 200),
        Flow::ImportDuty => Color32::from_rgb(190, 230, 90),
        Flow::Tolls => Color32::from_rgb(250, 190, 60),
        Flow::TollUpkeep => Color32::from_rgb(180, 110, 70),
        Flow::GridExport => Color32::from_rgb(120, 225, 245),
        Flow::RoadMaintenance => Color32::from_rgb(160, 130, 100),
        Flow::Fines => Color32::from_rgb(220, 150, 255),
    }
}

/// What a ledger line holds, in words: the answer to "what exactly is in
/// service upkeep".
pub fn flow_note(f: Flow) -> &'static str {
    match f {
        Flow::IncomeTax => "12% of every citizen's daily income, taken at midnight; by the payer's income class.",
        Flow::BusinessTax => "5% of every open business's takings, at midnight, by the land use it stands on; plus the levy on air-taxi fares.",
        Flow::BusFares => "Fares on each bus line as riders board, and ferry fares by terminal.",
        Flow::RailFares => "Train fares, by the station the rider boarded at.",
        Flow::Tickets => "Stadium game-night tickets, by stadium.",
        Flow::ParkingFines => "Tickets the parking wardens write.",
        Flow::ParkingFees => "Fees at the car parks the city runs, by car park.",
        Flow::PlantUpkeep => "Each power plant's and battery pack's running cost, paid at midnight.",
        Flow::GridImport => "Electricity bought from the regional grid when the plants and batteries fall short; the Power window shows when.",
        Flow::TransitUpkeep => "The buses on the lines and each depot, and each port or ferry terminal, paid at midnight.",
        Flow::RailUpkeep => "Each station and freight yard, paid at midnight.",
        Flow::Construction => "One-off costs of what the city builds: plants, services, depots, stations, ports, car parks, bridges.",
        Flow::ServiceUpkeep => "Each police, fire, health, school, dispatch, jail, trash, vertiport, monument and stadium building; the city's car parks; help called from the next town when ours were busy; less hospital bills the patients paid.",
        Flow::TouristTax => "12% on every hotel night visitors pay, by hotel.",
        Flow::SubwayFares => "Metro fares as riders board, by station.",
        Flow::SubwayUpkeep => "Each metro station and its tunnels, and every train run, paid at midnight.",
        Flow::ImportDuty => "The duty on every import from outside (the rate is in the Taxes window).",
        Flow::Tolls => "Tolls paid at the city's toll booths and plazas.",
        Flow::TollUpkeep => "Staff and upkeep of each toll booth and plaza, paid at midnight.",
        Flow::GridExport => "Surplus electricity sold to the regional grid (what the batteries had no room for), paid at midnight at the export price; the Power window shows when.",
        Flow::RoadMaintenance => "Road crews resurfacing worn streets, up to the day's allowance (the funding level in the Taxes window times what the network needs); the crews go to dear land first.",
        Flow::Fines => "Fines from busted street races and shut-down raves.",
    }
}

/// The compact view: treasury and yesterday's ledger as signed bars.
pub fn compact(ui: &mut egui::Ui, p: &mut UiCtx) {
    let e = &p.sim.world.economy;
    section(ui, "TREASURY");
    row(ui, "balance", warn_if(e.treasury < 0.0, format!("{:.0}", e.treasury)).strong());
    row(ui, "taxes so far today", format!("{:.0}", e.tax_today));
    row(ui, "plant upkeep", format!("{:.0}/day   imported power {:.0} today   exported {:.0} today", e.power.upkeep_today, e.power.import_cost_today, e.power.export_income_today));
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
    // [economy] what the city's businesses want
    super::pressure::compact(ui, &p.sim.world);
    hint(ui, "Expand (the corner button) for the ledger over time and what each line is made of.");
}

fn row(ui: &mut egui::Ui, label: &str, value: impl Into<egui::RichText>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).weak());
        ui.label(value.into());
    });
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
        painter.vline(mid, rect.y_range(), egui::Stroke::new(1.0, Color32::from_gray(90)));
        let half = rect.width() / 2.0 - 2.0;
        let len = (value.abs() / scale * half).max(if value == 0.0 { 0.0 } else { 1.0 });
        let (x0, x1, col) = if value >= 0.0 { (mid, mid + len, INCOME) } else { (mid - len, mid, WARN) };
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, rect.top() + 2.0), egui::pos2(x1, rect.bottom() - 2.0)), 1.0, col);
        ui.label(warn_if(value < 0.0, format!("{value:+.0}")).small());
    });
}

/// What the player asked for from inside the expanded view, applied after
/// the world is no longer borrowed.
#[derive(Default)]
struct Actions {
    select: Option<nd_sim::BuildingId>,
    fly: Option<Vec2>,
}

/// The expanded view: the sections in order.
pub fn expanded(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::span_picker(ui, Info::Economy);
    let mut act = Actions::default();
    {
        let w = &p.sim.world;
        let assets = p.assets;
        let zone = |lu: LandUse| zone_color32(assets.palette.zone_color(lu));
        let days = pane::days(&w.economy.history, span);
        headline(ui, w, &days);
        // [economy] what the city's businesses want, and why
        super::pressure::expanded(ui, w);
        treasury(ui, w, &days, span);
        flows_over_time(ui, &days);
        ledger_drill(ui, w, &days, &mut act);
        tax_take(ui, w, &days, &zone);
        // A taxes section (rates per sub-type) slots in here.
        trade(ui, w, &days);
    }
    if let Some(b) = act.select {
        *p.selection = Selection::Building(b);
    }
    if let Some(at) = act.fly {
        p.fly_to = Some(at);
    }
}

/// Treasury, yesterday's net, the average over the range and how long the
/// money lasts if the average is a loss.
fn headline(ui: &mut egui::Ui, w: &World, days: &[&DayRecord]) {
    let e = &w.economy;
    let n = days.len().max(1) as f32;
    let avg_net = days.iter().map(|d| d.net()).sum::<f32>() / n;
    let income = days.iter().map(|d| d.income()).sum::<f32>() / n;
    let spending = days.iter().map(|d| d.spending()).sum::<f32>() / n;
    let yesterday: f32 = e.ledger.yesterday.iter().sum();
    ui.horizontal_wrapped(|ui| {
        pane::kpi(ui, "treasury", warn_if(e.treasury < 0.0, format!("{:.0}", e.treasury)).color(if e.treasury < 0.0 { WARN } else { GOLD }));
        pane::kpi(ui, "net yesterday", warn_if(yesterday < 0.0, format!("{yesterday:+.0}")));
        pane::kpi(ui, &format!("avg net/day over {} d", days.len()), warn_if(avg_net < 0.0, format!("{avg_net:+.0}")));
        pane::kpi(ui, "avg income/day", egui::RichText::new(format!("{income:.0}")).color(INCOME));
        pane::kpi(ui, "avg spending/day", egui::RichText::new(format!("{spending:.0}")).color(WARN));
        if avg_net < 0.0 && e.treasury > 0.0 {
            pane::kpi(ui, "days of money left at that rate", egui::RichText::new(format!("{:.0}", e.treasury / -avg_net)).color(WARN));
        }
    });
}

fn treasury(ui: &mut egui::Ui, w: &World, days: &[&DayRecord], span: Span) {
    section(ui, "TREASURY");
    let h = &w.economy.history;
    if span == Span::Hours {
        let hours = pane::hours(h);
        pane::line_chart(ui, &pane::hour_labels(&hours), &[Series::new("treasury", GOLD, pane::hcol(&hours, |r| r.treasury))], 110.0, &|x| format!("{x:.0}"));
    } else {
        let series = [Series::new("treasury at midnight", GOLD, pane::col(days, |d| d.treasury))];
        pane::line_chart(ui, &pane::day_labels(days), &series, 110.0, &|x| format!("{x:.0}"));
    }
}

/// Every flow as a layer of a diverging stacked bar a day: income above
/// the axis, spending below, the net as a line.
fn flows_over_time(ui: &mut egui::Ui, days: &[&DayRecord]) {
    section(ui, "INCOME AND SPENDING BY FLOW, PER DAY");
    let used: Vec<Flow> = Flow::ALL.into_iter().filter(|f| days.iter().any(|d| d.flow(*f) != 0.0)).collect();
    let series: Vec<Series> = used.iter().map(|f| Series::new(f.label(), flow_color(*f), pane::col(days, |d| d.flow(*f)))).collect();
    pane::stacked_bars(ui, &pane::day_labels(days), &series, 170.0, true, None, &|x| format!("{x:+.0}"));
    let legend: Vec<(&str, Color32)> = used.iter().map(|f| (f.label(), flow_color(*f))).collect();
    pane::legend(ui, &legend);
    hint(ui, "Bars above the line are income, below are spending; the light line is the day's net. Hover a day for every figure.");
}

/// Which day the drill-down itemises; kept in egui memory.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum DrillDay {
    #[default]
    Yesterday,
    Today,
}

/// Every ledger line, opening onto what made it.
fn ledger_drill(ui: &mut egui::Ui, w: &World, days: &[&DayRecord], act: &mut Actions) {
    section(ui, "LEDGER, LINE BY LINE");
    let id = egui::Id::new("economy_drill_day");
    let mut which: DrillDay = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("itemise").small().weak());
        ui.selectable_value(&mut which, DrillDay::Yesterday, egui::RichText::new("yesterday").small());
        ui.selectable_value(&mut which, DrillDay::Today, egui::RichText::new("today so far").small());
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, which));
    let l = &w.economy.ledger;
    let (row, items) = match which {
        DrillDay::Yesterday => (&l.yesterday, &l.items_yesterday),
        DrillDay::Today => (&l.today, &l.items_today),
    };
    let n = days.len().max(1) as f32;
    let mut flows: Vec<Flow> = Flow::ALL.into_iter().filter(|f| row[f.index()] != 0.0 || days.iter().any(|d| d.flow(*f) != 0.0)).collect();
    flows.sort_by(|a, b| row[b.index()].abs().total_cmp(&row[a.index()].abs()));
    if flows.is_empty() {
        hint(ui, "Nothing on the books yet.");
        return;
    }
    for f in flows {
        let v = row[f.index()];
        let avg = days.iter().map(|d| d.flow(f)).sum::<f32>() / n;
        let title = egui::RichText::new(format!("{}   {:+.0}   (avg {:+.0}/day over {} d)", f.label(), v, avg, days.len())).color(if v < 0.0 { WARN } else { flow_color(f) });
        egui::CollapsingHeader::new(title).id_salt(("ledger_flow", f.index())).show(ui, |ui| {
            hint(ui, flow_note(f));
            let (lines, rest) = Ledger::items_of(items, row, f);
            item_table(ui, w, f, &lines, rest, v, act);
        });
    }
}

/// The items of one flow: name, what it is, postings, amount and its share
/// of the line as a bar; buildings get a button that selects them and
/// flies the camera there.
fn item_table(ui: &mut egui::Ui, w: &World, f: Flow, lines: &[&ItemEntry], rest: f32, total: f32, act: &mut Actions) {
    if lines.is_empty() && rest.abs() < 0.5 {
        hint(ui, "Nothing this day.");
        return;
    }
    let scale = lines.iter().map(|e| e.amount.abs()).fold(rest.abs(), f32::max).max(1e-3);
    egui::Grid::new(("ledger_items", f.index())).num_columns(5).spacing(egui::vec2(10.0, 2.0)).striped(true).show(ui, |ui| {
        for e in lines {
            let (name, what) = item_label(w, &e.item);
            if let Item::Building(b) = e.item {
                let gone = !w.buildings.contains_key(b);
                if ui.add_enabled(!gone, egui::Button::new(egui::RichText::new(name).small()).frame(false)).on_hover_text("Select it and fly there").clicked() {
                    act.select = Some(b);
                    act.fly = w.buildings.get(b).and_then(|bl| w.lots.get(bl.lot)).map(|l| Vec2::new(l.center.x, l.center.y));
                }
            } else {
                ui.label(egui::RichText::new(name).small());
            }
            ui.label(egui::RichText::new(what).small().weak());
            ui.label(egui::RichText::new(if e.count > 1 { format!("×{}", e.count) } else { String::new() }).small().weak());
            ui.label(warn_if(e.amount < 0.0, format!("{:+.0}", e.amount)).small());
            share_bar(ui, e.amount, scale, flow_color(f));
            ui.end_row();
        }
        if rest.abs() >= 0.5 {
            ui.label(egui::RichText::new("not itemised").small().italics());
            ui.label(egui::RichText::new("postings that named no item").small().weak());
            ui.label("");
            ui.label(warn_if(rest < 0.0, format!("{rest:+.0}")).small());
            share_bar(ui, rest, scale, Color32::from_gray(120));
            ui.end_row();
        }
    });
    if lines.len() > 1 {
        hint(ui, &format!("{} lines, {:+.0} in all", lines.len() + usize::from(rest.abs() >= 0.5), total));
    }
}

fn share_bar(ui: &mut egui::Ui, v: f32, scale: f32, col: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(90.0, 9.0), egui::Sense::hover());
    let len = (v.abs() / scale * r.width()).max(1.0);
    ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(len, r.height())), 1.0, col.gamma_multiply(0.8));
}

/// What a building is, for the drill-down's second column.
pub fn building_kind(b: &Building) -> String {
    if let Some(s) = b.service {
        return s.kind.tier_label(s.tier).to_string();
    }
    if let Some(k) = b.plant {
        return k.label().to_string();
    }
    if b.depot {
        return "bus depot".into();
    }
    if let Some(r) = &b.rail {
        return r.kind.label().to_string();
    }
    if let Some(k) = b.port {
        return k.label().to_string();
    }
    if b.is_parking() {
        return "car park".into();
    }
    format!("{} building", b.land_use.label().to_lowercase())
}

/// An item's name and kind.
pub fn item_label(w: &World, it: &Item) -> (String, String) {
    match it {
        Item::Building(id) => match w.buildings.get(*id) {
            Some(b) => (b.name.clone(), building_kind(b)),
            None => ("(gone)".into(), "demolished since".into()),
        },
        Item::Class(t) => (format!("{} earners", t.label()), "income class".into()),
        Item::LandUse(lu) => (lu.label().to_string(), "land use".into()),
        Item::Route(r) => (w.transit.routes.get(*r).map_or_else(|| "(removed line)".to_string(), |r| r.name.clone()), "bus line".into()),
        Item::Named(s) => (s.clone(), String::new()),
    }
}

/// Income tax by class and business tax by land use: yesterday's figures
/// and the range as stacked bars.
fn tax_take(ui: &mut egui::Ui, w: &World, days: &[&DayRecord], zone: &dyn Fn(LandUse) -> Color32) {
    section(ui, "TAX TAKE");
    let l = &w.economy.ledger;
    let mut series = Vec::new();
    for t in Tier::ALL {
        series.push(Series::new(format!("income tax, {} earners", t.label()), super::people::tier_color(t), pane::col(days, |d| d.income_tax_by_class[t as usize])));
    }
    for lu in LandUse::ALL {
        if days.iter().any(|d| d.business_tax_by_use[lu.index()] != 0.0) {
            series.push(Series::new(format!("business tax, {}", lu.label().to_lowercase()), zone(lu), pane::col(days, |d| d.business_tax_by_use[lu.index()])));
        }
    }
    pane::stacked_bars(ui, &pane::day_labels(days), &series, 120.0, false, None, &|x| format!("{x:.0}"));
    let legend: Vec<(&str, Color32)> = series.iter().map(|s| (s.name.as_str(), s.color)).collect();
    pane::legend(ui, &legend);
    // Yesterday, as figures.
    let of = |flow: Flow, pick: &dyn Fn(&Item) -> bool| l.items_yesterday.iter().filter(|e| e.flow == flow && pick(&e.item)).map(|e| e.amount).sum::<f32>();
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("yesterday, income tax").small().weak());
        for t in Tier::ALL {
            let x = of(Flow::IncomeTax, &|i| *i == Item::Class(t));
            ui.label(egui::RichText::new(format!("{} {:.0}", t.label(), x)).small().color(super::people::tier_color(t)));
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("yesterday, business tax").small().weak());
        for lu in LandUse::ALL {
            let x = of(Flow::BusinessTax, &|i| *i == Item::LandUse(lu));
            if x != 0.0 {
                ui.label(egui::RichText::new(format!("{} {:.0}", lu.label().to_lowercase(), x)).small());
            }
        }
        let other = l.yesterday_of(Flow::BusinessTax) - l.items_yesterday.iter().filter(|e| e.flow == Flow::BusinessTax && matches!(e.item, Item::LandUse(_))).map(|e| e.amount).sum::<f32>();
        if other.abs() >= 0.5 {
            ui.label(egui::RichText::new(format!("other (air-taxi levy) {other:.0}")).small());
        }
    });
}

fn trade(ui: &mut egui::Ui, w: &World, days: &[&DayRecord]) {
    section(ui, "TRADE");
    let m = &w.economy.market;
    hint(ui, &format!("today so far: imports {:.0} · exports {:.0} · building materials {:.0} · {} deals", m.imports_today, m.exports_today, w.economy.materials_today, m.deals_today));
    let series = [
        Series::new("imports", WARN, pane::col(days, |d| d.imports)),
        Series::new("exports", INCOME, pane::col(days, |d| d.exports)),
        Series::new("building materials", Color32::from_rgb(150, 150, 165), pane::col(days, |d| d.materials)),
    ];
    pane::line_chart(ui, &pane::day_labels(days), &series, 110.0, &|x| format!("{x:.0}"));
    pane::legend(ui, &series.iter().map(|s| (s.name.as_str(), s.color)).collect::<Vec<_>>());
    hint(ui, "Value of goods bought from and sold out of town by the city's businesses (not the treasury's money).");
}
