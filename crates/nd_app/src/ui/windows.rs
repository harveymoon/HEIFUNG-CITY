//! The info windows: draggable, closable `egui::Window`s opened from the top
//! bar. egui remembers each window's position in its own memory; whether a
//! window is open lives in the same memory, so the UI carries no state of
//! its own for them. Every window has a compact and an expanded view
//! (`ui/pane.rs` explains the split). Each window's body is one function below, ported from
//! the sections of the old side panel.

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy_egui::egui;
use nd_sim::{LandUse, Resource as Good};

use super::tools::warn_if;
use super::{detail, economy, help, hint, neon, pane, people, saves, section, zone_color32, UiCtx, WARN};
use crate::camera::CameraMode;

/// The windows the top bar can open, in toggle order. `Saves` and `Help`
/// come after a separator in the bar, `Dev` after another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Info {
    City,
    Economy,
    /// [tax] Rates per land use and sub-type, import duty, toll booths (ui/taxes.rs).
    Taxes,
    Market,
    People,
    Power,
    Transit,
    /// [lines] Bus lines one by one: a strip diagram of the picked line's
    /// stops, legs and buses, with its editing controls (ui/lines.rs).
    Lines,
    /// 911 calls, live and on record (ui/emergencies.rs).
    Emergencies,
    /// The View window (once "Land"): the map layer picker, land value,
    /// lighting. Titled "View"; the API still takes "land".
    Land,
    Saves,
    Help,
    Dev,
    /// [districts] Districts, their policies and figures; tourism.
    Districts,
}

impl Info {
    pub const ALL: [Info; 14] = [Info::City, Info::Economy, Info::Taxes, Info::Market, Info::People, Info::Power, Info::Transit, Info::Lines, Info::Emergencies, Info::Land, Info::Districts, Info::Saves, Info::Help, Info::Dev];

    pub fn title(self) -> &'static str {
        match self {
            Info::City => "City",
            Info::Economy => "Economy",
            Info::Taxes => "Taxes",
            Info::Market => "Market",
            Info::People => "People",
            Info::Power => "Power",
            Info::Transit => "Transit",
            Info::Lines => "Lines",
            Info::Emergencies => "Emergencies",
            Info::Land => "View",
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
            Info::Lines => 430.0,
            Info::Taxes => 470.0,
            Info::Dev => 300.0,
            // [identity] Room for the city name row and the grouped list.
            Info::Saves => 460.0,
            _ => 380.0,
        }
    }

    /// Whether the window has an expanded view. Saves, Help and Dev are
    /// tools rather than reports and have nothing to chart.
    pub fn expandable(self) -> bool {
        !matches!(self, Info::Saves | Info::Help | Info::Dev | Info::Lines)
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
///
/// Each draws its own header (`pane::header`: title, expand, close) in
/// place of egui's title bar, so the expand button sits in the same spot on
/// every window; the window still drags by any empty part of it. The
/// expanded view is a separate egui window (its own id), so each view keeps
/// its own size and place; it first opens where the compact one stood.
pub fn show_all(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    // A window closed while expanded gives the expansion up.
    if let Some(big) = pane::expanded(ctx).filter(|i| !i.is_open(ctx)) {
        pane::set_expanded(ctx, big, false);
    }
    for (i, info) in Info::ALL.into_iter().enumerate() {
        if !info.is_open(ctx) {
            continue;
        }
        let big = info.expandable() && pane::expanded(ctx) == Some(info);
        let cascade = content.left_top() + egui::vec2(12.0 + 28.0 * i as f32, 12.0 + 30.0 * i as f32);
        let corner_id = info.id().with("compact_corner");
        let window = egui::Window::new(info.title()).title_bar(false).constrain_to(content).max_height(content.height() - 24.0);
        let window = if big {
            let pos = ctx.data(|d| d.get_temp::<egui::Pos2>(corner_id)).unwrap_or(cascade);
            let size = egui::vec2(EXPANDED_WIDTH.min(content.width() - 24.0), EXPANDED_HEIGHT.min(content.height() - 24.0));
            window.id(info.id().with("expanded")).default_pos(pos).default_size(size).resizable(true)
        } else {
            window.id(info.id().with("window")).default_pos(cascade).default_width(info.width())
        };
        let mut keep = true;
        let shown = window.show(ctx, |ui| {
            keep = pane::header(ui, info, info.expandable());
            let scroll = egui::ScrollArea::vertical().id_salt(("info_scroll", info.title(), big));
            let scroll = if big { scroll.auto_shrink([false, false]) } else { scroll };
            scroll.show(ui, |ui| body(ui, p, info, big));
        });
        if let (Some(r), false) = (shown, big) {
            ctx.data_mut(|d| d.insert_temp(corner_id, r.response.rect.left_top()));
        }
        if !keep {
            info.set_open(ctx, false);
            pane::set_expanded(ctx, info, false);
        }
    }
}

/// Where an expanded window starts, in points; it resizes from there.
const EXPANDED_WIDTH: f32 = 820.0;
const EXPANDED_HEIGHT: f32 = 680.0;

fn body(ui: &mut egui::Ui, p: &mut UiCtx, info: Info, big: bool) {
    match (info, big) {
        (Info::City, false) => city(ui, p),
        (Info::City, true) => detail::city(ui, p),
        (Info::Economy, false) => economy::compact(ui, p),
        (Info::Economy, true) => economy::expanded(ui, p),
        (Info::Market, false) => market(ui, p),
        (Info::Market, true) => detail::market(ui, p),
        (Info::People, _) => people::window(ui, p, big),
        (Info::Power, false) => power(ui, p),
        (Info::Power, true) => detail::power(ui, p),
        (Info::Transit, false) => transit(ui, p),
        (Info::Transit, true) => detail::transit(ui, p),
        (Info::Lines, _) => super::lines::window(ui, p),
        (Info::Emergencies, _) => super::emergencies::window(ui, p, big),
        (Info::Land, false) => land(ui, p),
        (Info::Land, true) => detail::land(ui, p),
        (Info::Saves, _) => saves::panel(ui, p.sim, p.tools, p.selection, p.plan, p.saves, p.flags),
        (Info::Help, _) => help_window(ui),
        (Info::Dev, _) => dev(ui, p),
        (Info::Districts, _) => super::districts::window(ui, p),
        (Info::Taxes, _) => super::taxes::window(ui, p),
    }
}

/// A labelled figure on one line: the label weak, the value plain.
fn row(ui: &mut egui::Ui, label: &str, value: impl Into<egui::RichText>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).weak());
        ui.label(value.into());
    });
}

pub(super) fn city(ui: &mut egui::Ui, p: &mut UiCtx) {
    // [identity] The city's name, renamable.
    super::saves::city_name_row(ui, p.sim, p.flags);
    let w = &p.sim.world;
    let e = &w.economy;
    let sites = w.buildings.values().filter(|b| b.construction.is_some()).count();
    section(ui, "BUILDINGS & LOTS");
    row(ui, "buildings", format!("{}   under construction {}", w.buildings.len() - sites, sites));
    row(ui, "lots", format!("{} ({} vacant, {} unusable)", w.lots.len(), w.lots.values().filter(|l| l.building.is_none() && l.usable()).count(), w.lots.values().filter(|l| l.landlocked && l.park.is_none() && l.land_use != LandUse::Park).count()));
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
        ui.add(egui::ProgressBar::new(e.demand_for(lu)).text(lu.label()).fill(col.gamma_multiply(0.7)).desired_height(18.0));
    }
}

pub(super) fn power(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let pw = &w.economy.power;
    section(ui, "GRID");
    ui.add(egui::ProgressBar::new(pw.served).text(format!("served {:.0}%", pw.served * 100.0)).fill(if pw.served < 0.98 { WARN.gamma_multiply(0.8) } else { egui::Color32::from_rgb(110, 200, 140).gamma_multiply(0.8) }).desired_height(18.0));
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
        section(ui, "BATTERIES");
        let level = if pw.battery_capacity_mwh > 0.0 { pw.battery_charge_mwh / pw.battery_capacity_mwh } else { 0.0 };
        let state = if pw.battery_out_mw > 0.0 { format!("delivering {:.0} MW", pw.battery_out_mw) } else if pw.battery_in_mw > 0.0 { format!("charging {:.0} MW", pw.battery_in_mw) } else { "holding".to_string() };
        ui.add(egui::ProgressBar::new(level).text(format!("{:.0} of {:.0} MWh · {state}", pw.battery_charge_mwh, pw.battery_capacity_mwh)).fill(detail::BATTERY.gamma_multiply(0.8)).desired_height(18.0));
    }
    let rows = w.plant_rows();
    if !rows.is_empty() {
        section(ui, "PLANTS");
        detail::plant_table(ui, w, &rows, false);
    }
    section(ui, "COSTS");
    row(ui, "upkeep", format!("{:.0}/day (plants and batteries)", pw.upkeep_today));
    row(ui, "grid import", format!("{:.0} today (up to {:.0} MW, now {:.2}/MWh)", pw.import_cost_today, nd_sim::power::GRID_IMPORT_MW, pw.import_price));
    row(ui, "grid export", format!("+{:.0} today, +{:.0} yesterday (up to {:.0} MW, now {:.2}/MWh)", pw.export_income_today, w.economy.ledger.yesterday_of(nd_sim::economy::Flow::GridExport), nd_sim::power::GRID_EXPORT_MW, pw.export_price));
    // [grid-weather] Time-of-use: the next day's demand and price.
    section(ui, "NEXT 24 HOURS");
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
    section(ui, "WEATHER");
    let x = &w.weather;
    let spell = if x.spell == nd_sim::power::Spell::Normal { String::new() } else { format!("   {} for {} more hours", x.spell.label(), x.spell_hours) };
    row(ui, x.label(), format!("{:.0} °C   cloud {:.0}%   wind {:.1} m/s{spell}", x.temp_c, x.cloud * 100.0, x.wind_ms()));
    hint(ui, &format!("{} climate. Solar dims under cloud and in great heat, and stops at night; wind farms follow the wind at their site and brake above {:.0} m/s in a storm. Heat waves and cold snaps raise demand (air conditioning, heating). Surplus charges the batteries, then is sold; a shortfall drains them, then is bought.", x.climate.label(), nd_sim::weather::CUT_OUT_MS));
}

pub(super) fn transit(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let t = &w.economy.transit;
    section(ui, "BUSES");
    if t.depots == 0 {
        ui.label(egui::RichText::new("No depot: buses cannot run. The Buses tool places one.").small().color(WARN));
    }
    row(ui, "depots", format!("{} for {} buses   {} out on the lines", t.depots, t.depot_capacity, t.buses));
    row(ui, "riders", format!("{} waiting   {} riding   {} today", t.waiting, t.riding, t.riders_today));
    row(ui, "money", format!("fares {:.0} today   upkeep {:.0}/day", t.fares_today, t.upkeep_today));
    if ui.small_button("Lines: stops, times and ridership…").on_hover_text("Open the Lines window to tune a line stop by stop").clicked() {
        Info::Lines.set_open(ui.ctx(), true);
    }
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
    // [subway]
    let s = &w.subway.stats;
    section(ui, "METRO");
    if s.lines == 0 {
        hint(ui, "No metro line yet: place metro stations (Build window, Transit, Metro), then Metro lines joins them station by station.");
    }
    row(ui, "network", format!("{} station{}   {} line{}   {:.1} km   {} train{} running", s.stations, if s.stations == 1 { "" } else { "s" }, s.lines, if s.lines == 1 { "" } else { "s" }, s.tunnel_m / 1000.0, s.trains, if s.trains == 1 { "" } else { "s" }));
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

/// [taxis] The Transit window's taxi and cargo hub sections.
fn taxis_and_freight(ui: &mut egui::Ui, w: &nd_sim::World) {
    let t = &w.taxis;
    let s = &t.stats;
    let k = t.counts();
    section(ui, "TAXIS");
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
        section(ui, "CARGO HUBS");
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

/// The View window: the map layer (one at a time, with its key in the
/// legend), what else is drawn, land value and class mix, and the lighting.
pub(super) fn land(ui: &mut egui::Ui, p: &mut UiCtx) {
    section(ui, "MAP LAYER");
    hint(ui, "One layer at a time, its key at the bottom right of the map. The Layer button in the top bar picks it too; V, O, P toggle theirs, L steps through the heat maps, K through the land layers.");
    super::topbar::layer_list(ui, p.overlays, true);
    section(ui, "ALSO DRAWN");
    ui.checkbox(&mut p.tools.show_routes, "Bus lines whatever the tool");
    ui.checkbox(&mut p.overlays.projects, "Other pending projects, dim");
    ui.checkbox(&mut p.lighting.heat, "Drone heat vision [H]");
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
    graphics_setting(ui, p);
    super::theme::scale_setting(ui);
}

/// [look] The graphics setting: Auto holds about 60 fps by stepping the
/// effects down and up; a tier pins them.
fn graphics_setting(ui: &mut egui::Ui, p: &mut UiCtx) {
    section(ui, "GRAPHICS");
    ui.horizontal(|ui| {
        for q in crate::render::QualitySetting::ALL {
            if ui.selectable_label(p.lighting.quality == q, q.label()).clicked() {
                p.lighting.quality = q;
            }
        }
    });
    let g = &p.stats.graphics;
    let now = if p.lighting.quality == crate::render::QualitySetting::Auto { format!("Auto, at {} now", g.tier.label()) } else { g.tier.label().to_string() };
    hint(ui, &format!("{now} · {:.1} ms a frame. Auto steps ambient occlusion, volumetric fog, anti-aliasing and shadow detail down to hold 60 fps, and back up when there is room.", g.frame_ms));
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
    // [look] The graphics tier in force and why, the frame time it is held to.
    let g = &p.stats.graphics;
    hint(ui, &format!("graphics: {} ({}) · {:.1} ms/frame, sim {:.1} ms · {} auto steps · {}", g.tier.label(), p.lighting.quality.label(), g.frame_ms, g.sim_ms, g.steps, g.reason));
    hint(ui, &g.effects);
    if let Some(err) = &p.assets.last_error {
        ui.label(egui::RichText::new(err).small().color(egui::Color32::from_rgb(255, 60, 90)));
    }
    hint(ui, help::ASSETS_RELOAD);
}
