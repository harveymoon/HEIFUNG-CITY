//! City Data's Economy tab: Overview (the treasury over time and what the
//! city's businesses want), Cash flow (income and spending by flow, day by
//! day, and yesterday's books as signed bars), Ledger (every ledger line
//! broken into the buildings, lines, classes or land uses that made it, from
//! the sim's itemised ledger) and Taxes (the take by class and land use,
//! then the rates, from `ui/taxes.rs`).
//!
//! Each page is a column of independent sections, each a function taking
//! the world and the range picked in the view's sub-tab row.

use bevy::prelude::Vec2;
use bevy_egui::egui::{self, Color32};
use nd_sim::economy::{Flow, Item, ItemEntry, Ledger};
use nd_sim::history::DayRecord;
use nd_sim::{Building, LandUse, Tier, World};

use super::pane::{self, Series, Span};
use super::tools::warn_if;
use super::pane::{heading, row};
use super::{hint, UiCtx, GOLD, WARN};
use crate::tools::Selection;

const INCOME: Color32 = super::theme::GOOD;

/// A fixed colour per flow: greens and blues for income, reds, oranges and
/// purples for spending, so the stacked chart reads by temperature.
pub fn flow_color(f: Flow) -> Color32 {
    match f {
        Flow::IncomeTax => super::theme::GOOD,
        Flow::BusinessTax => Color32::from_rgb(70, 170, 210),
        Flow::BusFares => Color32::from_rgb(120, 230, 200),
        Flow::RailFares => Color32::from_rgb(150, 200, 255),
        Flow::Tickets => Color32::from_rgb(230, 210, 110),
        Flow::ParkingFines => Color32::from_rgb(200, 170, 250),
        Flow::ParkingFees => Color32::from_rgb(170, 230, 120),
        Flow::PlantUpkeep => super::WARN,
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
        Flow::Gifts => Color32::from_rgb(255, 210, 90),
        Flow::RoadMaintenance => Color32::from_rgb(160, 130, 100),
        Flow::Fines => Color32::from_rgb(220, 150, 255),
        Flow::PropertyTax => Color32::from_rgb(100, 200, 120),
        Flow::DistrictLevies => Color32::from_rgb(200, 140, 230),
        Flow::DistrictSpending => Color32::from_rgb(150, 110, 200),
        Flow::WasteDisposal => Color32::from_rgb(150, 120, 70),
        Flow::RecyclingSales => Color32::from_rgb(90, 200, 110),
        Flow::AirportFees => Color32::from_rgb(130, 180, 255),
        Flow::AirportUpkeep => Color32::from_rgb(120, 90, 220),
    }
}

/// What a ledger line holds, in words: the answer to "what exactly is in
/// service upkeep".
fn flow_note(f: Flow) -> &'static str {
    match f {
        Flow::IncomeTax => "12% of every citizen's daily income, taken at midnight; by the payer's income class.",
        Flow::BusinessTax => "5% of every open business's takings, at midnight, by the land use it stands on; plus the levy on air-taxi fares.",
        Flow::BusFares => "Fares on each bus line as riders board, and ferry fares by terminal.",
        Flow::RailFares => "Train fares, by the station the rider boarded at.",
        Flow::Tickets => "Stadium game-night tickets and admissions to the landmark attractions (museums, the zoo, the pier...), by building.",
        Flow::ParkingFines => "Tickets the parking wardens write.",
        Flow::ParkingFees => "Fees at the car parks the city runs, by car park.",
        Flow::PlantUpkeep => "Each power plant's and battery pack's running cost, paid at midnight.",
        Flow::GridImport => "Electricity bought from the regional grid when the plants and batteries fall short; Services, Power in City Data shows when.",
        Flow::TransitUpkeep => "The buses on the lines and each depot, and each port or ferry terminal, paid at midnight.",
        Flow::RailUpkeep => "Each station and freight yard, paid at midnight.",
        Flow::Construction => "One-off costs of what the city builds: plants, services, depots, stations, ports, car parks, bridges.",
        Flow::ServiceUpkeep => "Each police, fire, health, school, dispatch, jail, trash, vertiport, monument and stadium building; the city's car parks; help called from the next town when ours were busy; less hospital bills the patients paid.",
        Flow::TouristTax => "12% on every hotel night visitors pay, by hotel.",
        Flow::SubwayFares => "Metro fares as riders board, by station.",
        Flow::SubwayUpkeep => "Each metro station and its tunnels, and every train run, paid at midnight.",
        Flow::ImportDuty => "The duty on every import from outside (the rate is in City Data, Economy, Taxes).",
        Flow::Tolls => "Tolls paid at the city's toll booths and plazas.",
        Flow::TollUpkeep => "Staff and upkeep of each toll booth and plaza, paid at midnight.",
        Flow::GridExport => "Surplus electricity sold to the regional grid (what the batteries had no room for), paid at midnight at the export price; Services, Power in City Data shows when.",
        Flow::Gifts => "Money the city gave residents' households (the person pane's and favourites' \"gift $1000\"), by who got it.",
        Flow::RoadMaintenance => "Road crews resurfacing worn streets, up to the day's allowance (the funding level in City Data, Economy, Taxes, times what the network needs); the crews go to dear land first.",
        Flow::Fines => "Fines from busted street races and shut-down raves.",
        Flow::PropertyTax => "10% of every rent collected, paid by the building's owner (a resident, a company, a developer, the independent landlords), by building. Economy, Money flow shows where the rest of the rent goes.",
        Flow::DistrictLevies => "Each district's own levies, set by its story and fine-tuned on City Data, Districts: business rates, noise and alcohol levies, pollution levies and bed levies, by district and policy.",
        Flow::DistrictSpending => "What districts' policies pay out: rent support to residents, heritage grants to owners and business-rate relief, by district and policy.",
        Flow::WasteDisposal => "Getting rid of the trash, by the tonne, paid at midnight: tipped at each landfill, burnt in each incinerator, sorted at each recycling plant, shipped from each transfer station, and the dearest, the lorries that haul the depots' yards out of town. Services, Trash in City Data shows where it all went.",
        Flow::RecyclingSales => "Metal, plastics, paper and glass the recycling plants recover: sold to the scrap yards and works in town that are short, the rest out of town, by plant.",
        Flow::AirportFees => "Landing charges for every plane, a fee for every passenger flying out and the handling fee on air cargo, by airport.",
        Flow::AirportUpkeep => "Running each airport (staff, runways, the tower, the terminal), paid at midnight.",
    }
}

/// Economy, Overview: the headline figures, the treasury over time, today
/// so far, and what the city's businesses want.
pub fn overview(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::data_span(ui.ctx());
    let w = &p.sim.world;
    let e = &w.economy;
    let days = pane::days(&e.history, span);
    headline(ui, w, &days);
    treasury(ui, w, &days, span);
    heading(ui, "Today so far");
    let today: f32 = e.ledger.today.iter().sum();
    row(ui, "net", warn_if(today < 0.0, format!("{today:+.0}")));
    row(ui, "taxes", format!("{:.0}", e.tax_today));
    row(ui, "power", format!("plant upkeep {:.0}/day · imported {:.0} · exported {:.0}", e.power.upkeep_today, e.power.import_cost_today, e.power.export_income_today));
    hint(ui, "Cash flow has every flow day by day; Ledger opens each line onto what made it.");
    // [economy] what the city's businesses want, and why
    super::pressure::wanted(ui, w, &mut p.flags.pressure_where, &mut p.fly_to);
}

/// Economy, Cash flow: every flow over the range, then yesterday's books.
pub fn cash_flow(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::data_span(ui.ctx());
    let w = &p.sim.world;
    let e = &w.economy;
    let days = pane::days(&e.history, span);
    let income: f32 = e.ledger.yesterday.iter().filter(|x| **x > 0.0).sum();
    let spending: f32 = e.ledger.yesterday.iter().filter(|x| **x < 0.0).sum();
    let today: f32 = e.ledger.today.iter().sum();
    pane::cards(ui, |ui| {
        pane::kpi(ui, "treasury", warn_if(e.treasury < 0.0, format!("{:.0}", e.treasury)).color(if e.treasury < 0.0 { WARN } else { GOLD }));
        pane::kpi(ui, "income yesterday", egui::RichText::new(format!("{income:.0}")).color(INCOME));
        pane::kpi(ui, "spending yesterday", egui::RichText::new(format!("{spending:.0}")).color(WARN));
        pane::kpi(ui, "net yesterday", warn_if(income + spending < 0.0, format!("{:+.0}", income + spending)));
        pane::kpi(ui, "net today so far", warn_if(today < 0.0, format!("{today:+.0}")));
    });
    flows_over_time(ui, &days);
    heading(ui, "Yesterday's books");
    let entries = e.ledger.yesterday_sorted();
    if entries.is_empty() {
        hint(ui, "No books yet: the ledger closes at midnight.");
        return;
    }
    let scale = entries.iter().map(|(_, x)| x.abs()).fold(1.0, f32::max);
    for (flow, x) in &entries {
        signed_bar(ui, flow.label(), *x, scale, flow_color(*flow));
    }
    let net: f32 = entries.iter().map(|(_, x)| x).sum();
    ui.separator();
    // [fix/review-app] The net is a sum: it can outgrow the largest line.
    signed_bar(ui, "net", net, scale.max(net.abs()), if net < 0.0 { WARN } else { INCOME });
}

/// One ledger line: label, a bar growing right for income and left for
/// spending from a centre axis, and the figure. Drawn with the painter so
/// the sign reads at a glance.
fn signed_bar(ui: &mut egui::Ui, label: &str, value: f32, scale: f32, col: Color32) {
    ui.horizontal(|ui| {
        ui.add_sized(egui::vec2(190.0, 18.0), egui::Label::new(egui::RichText::new(label).color(pane::CAPTION)).truncate());
        let (resp, painter) = ui.allocate_painter(egui::vec2(360.0, 14.0), egui::Sense::hover());
        let rect = resp.rect;
        let mid = rect.center().x;
        painter.vline(mid, rect.y_range(), egui::Stroke::new(1.0, Color32::from_gray(90)));
        let half = rect.width() / 2.0 - 2.0;
        let len = (value.abs() / scale * half).max(if value == 0.0 { 0.0 } else { 1.0 }).min(half);
        let (x0, x1) = if value >= 0.0 { (mid, mid + len) } else { (mid - len, mid) };
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, rect.top() + 2.0), egui::pos2(x1, rect.bottom() - 2.0)), 1.0, col);
        ui.label(warn_if(value < 0.0, format!("{value:+.0}")));
    });
}

/// What the player asked for from inside a page, applied after the world
/// is no longer borrowed.
#[derive(Default)]
struct Actions {
    select: Option<nd_sim::BuildingId>,
    fly: Option<Vec2>,
}

/// Economy, Ledger: every line, opening onto what made it.
pub fn ledger(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::data_span(ui.ctx());
    let mut act = Actions::default();
    {
        let w = &p.sim.world;
        let days = pane::days(&w.economy.history, span);
        headline(ui, w, &days);
        ledger_drill(ui, w, &days, &mut act);
    }
    if let Some(b) = act.select {
        *p.selection = Selection::Building(b);
    }
    if let Some(at) = act.fly {
        p.fly_to = Some(at);
    }
}

/// Economy, Taxes: the take over time, then the rates (`ui/taxes.rs`).
pub fn taxes(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::data_span(ui.ctx());
    {
        let w = &p.sim.world;
        let l = &w.economy.ledger;
        let taxes = [Flow::IncomeTax, Flow::BusinessTax, Flow::TouristTax, Flow::ImportDuty, Flow::Tolls];
        let yesterday: f32 = taxes.iter().map(|f| l.yesterday_of(*f)).sum();
        let today: f32 = taxes.iter().map(|f| l.today_of(*f)).sum();
        let all: f32 = l.yesterday.iter().filter(|x| **x > 0.0).sum();
        pane::cards(ui, |ui| {
            pane::kpi(ui, "taxes yesterday", egui::RichText::new(format!("{yesterday:.0}")).color(GOLD));
            pane::kpi(ui, "taxes today so far", egui::RichText::new(format!("{today:.0}")));
            pane::kpi(ui, "share of yesterday's income", egui::RichText::new(if all > 0.0 { format!("{:.0}%", yesterday / all * 100.0) } else { "-".into() }));
            pane::kpi(ui, "import duty rate", egui::RichText::new(format!("{:.0}%", w.economy.taxes.import_duty * 100.0)));
        });
        let assets = p.assets;
        let zone = |lu: LandUse| super::theme::rgb_f(assets.palette.zone_color(lu));
        let days = pane::days(&w.economy.history, span);
        tax_take(ui, w, &days, &zone);
    }
    super::taxes::window(ui, p);
}

/// Treasury, yesterday's net, the average over the range and how long the
/// money lasts if the average is a loss.
fn headline(ui: &mut egui::Ui, w: &World, days: &[&DayRecord]) {
    let e = &w.economy;
    let n = days.len().max(1) as f32;
    // `+ 0.0`: an empty f32 sum is -0.0, which prints "-0".
    let avg_net = days.iter().map(|d| d.net()).sum::<f32>() / n + 0.0;
    let income = days.iter().map(|d| d.income()).sum::<f32>() / n + 0.0;
    let spending = days.iter().map(|d| d.spending()).sum::<f32>() / n + 0.0;
    let yesterday: f32 = e.ledger.yesterday.iter().sum::<f32>() + 0.0;
    pane::cards(ui, |ui| {
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
    heading(ui, "Treasury");
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
    heading(ui, "Income and spending by flow, per day");
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
    heading(ui, "Ledger, line by line");
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
    // [pt-ux] Sortable by name, kind, postings or amount; "not itemised"
    // stays last.
    let key = |e: &&ItemEntry, c: usize| match c {
        0 => super::pane::SortKey::text(item_label(w, &e.item).0),
        1 => super::pane::SortKey::text(item_label(w, &e.item).1),
        2 => super::pane::SortKey::num(e.count as f64),
        3 => super::pane::SortKey::num(e.amount),
        _ => super::pane::SortKey::None,
    };
    let heads = [("item", ""), ("what", ""), ("postings", ""), ("amount", ""), ("share", "")];
    super::pane::sortable_foot(
        ui,
        ("ledger_items", f.index()),
        &heads,
        lines,
        key,
        |ui, e| {
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
        },
        |ui| {
            if rest.abs() >= 0.5 {
                ui.label(egui::RichText::new("not itemised").small().italics());
                ui.label(egui::RichText::new("postings that named no item").small().weak());
                ui.label("");
                ui.label(warn_if(rest < 0.0, format!("{rest:+.0}")).small());
                share_bar(ui, rest, scale, Color32::from_gray(120));
                ui.end_row();
            }
        },
    );
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
fn building_kind(b: &Building) -> String {
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
fn item_label(w: &World, it: &Item) -> (String, String) {
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
    heading(ui, "Tax take");
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
