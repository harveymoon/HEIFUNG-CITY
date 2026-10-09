//! [train-inspect] The inspector on a selected train: what it is (headcode,
//! passenger or freight, coaches or wagons, its colour), what it is doing
//! (running, standing at a station, held at a signal or at the branch
//! switch), the stops it has made and those still to come with about when,
//! and what is aboard: the passengers, each with where they get off, where
//! they are going and why (a click selects them), or the freight, each load
//! with where it comes from, where it goes and what it is for. Metro trains
//! show the same for their line.

use bevy_egui::egui;
use nd_sim::rail::{self, FreightLoad, Train, TrainKind, TrainState};
use nd_sim::transit::TransitLeg;
use nd_sim::{BuildingId, CitizenId, World};

use super::pane::tag;
use super::{hint, section, GOLD, IVORY, WARN};
use crate::assets::GameAssets;
use crate::tools::Selection;
use crate::train_select::{body_rgb, glow_rgb, headcode, metro_code};

const RUNNING: egui::Color32 = super::theme::GOOD;
const CYAN: egui::Color32 = egui::Color32::from_rgb(120, 220, 255);

fn name_of(w: &World, b: BuildingId) -> String {
    w.buildings.get(b).map_or_else(|| "a stop that is gone".into(), |b| b.name.clone())
}

/// "in 4 min (14:32)" for a call `minutes` from now.
fn eta(w: &World, minutes: f32) -> String {
    let at = w.clock.tick + minutes.round().max(0.0) as u64;
    let when = super::fmt::hhmm(at);
    if minutes < 1.0 {
        format!("arriving now ({when})")
    } else {
        format!("in ~{:.0} min ({when})", minutes)
    }
}

/// The inspector body for `Selection::Train` / `Selection::Metro`.
pub fn show(ui: &mut egui::Ui, w: &World, selection: &mut Selection, assets: &GameAssets) {
    let picked = match *selection {
        Selection::Train(id) => {
            let Some(t) = w.rail.trains.get(id) else {
                *selection = Selection::None;
                return;
            };
            surface(ui, w, id, t, assets)
        }
        Selection::Metro(id) => {
            let Some(t) = w.subway.trains.get(id) else {
                *selection = Selection::None;
                return;
            };
            metro(ui, w, id, t)
        }
        _ => None,
    };
    if let Some(s) = picked {
        *selection = s;
    }
}

fn surface(ui: &mut egui::Ui, w: &World, id: nd_sim::TrainId, t: &Train, assets: &GameAssets) -> Option<Selection> {
    let mut picked = None;
    let body = body_rgb(&assets.palette.car_colors, t.color);
    let glow = super::theme::rgb_gamma(glow_rgb(body).map(|x| x / 3.0));
    let (kind, cars) = match t.kind {
        TrainKind::Passenger => ("Passenger train", format!("{} coach{}", t.cars, if t.cars == 1 { "" } else { "es" })),
        TrainKind::Freight => ("Freight train", format!("{} wagon{}", t.cars, super::fmt::s(t.cars))),
    };
    ui.label(egui::RichText::new(format!("{kind} {}", headcode(id, t.kind))).font(crate::ui::theme::lead()).color(IVORY).strong());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        tag(ui, if t.kind == TrainKind::Passenger { "Passengers" } else { "Freight" }, if t.kind == TrainKind::Passenger { CYAN } else { GOLD });
        tag(ui, &format!("locomotive + {cars}"), egui::Color32::from_gray(170));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, super::theme::rgb_gamma(body));
        ui.painter().rect_stroke(rect, 3.0, egui::Stroke::new(1.5, glow), egui::StrokeKind::Outside);
        hint(ui, "livery");
    });
    // Where it runs: the whole line, end to end.
    let (from_side, to_side) = ends(t);
    ui.label(egui::RichText::new(format!("Mainline service from the {from_side} edge of the map to the {to_side}")).small().weak());
    if let (Some(a), Some(b)) = (t.stops.first(), t.stops.last()) {
        let text = if a.0 == b.0 { format!("Calling at {}", name_of(w, a.0)) } else { format!("{} → {}  ·  {} stops", name_of(w, a.0), name_of(w, b.0), t.stops.len()) };
        ui.label(egui::RichText::new(text).color(CYAN).strong());
    }

    // At a glance: who or what is aboard (the detail is further down).
    match t.kind {
        TrainKind::Passenger => {
            ui.label(egui::RichText::new(format!("{} of {} seats taken", t.passengers.len(), rail::TRAIN_CAPACITY)).color(IVORY));
        }
        TrainKind::Freight => {
            if let Some(m) = w.freight_manifest(id) {
                let mut kinds: Vec<(nd_sim::market::Resource, f32)> = Vec::new();
                for l in &m.loads {
                    match kinds.iter_mut().find(|(r, _)| *r == l.resource) {
                        Some(k) => k.1 += l.amount,
                        None => kinds.push((l.resource, l.amount)),
                    }
                }
                kinds.sort_by(|a, b| b.1.total_cmp(&a.1));
                let text = if kinds.is_empty() {
                    if m.exports_aboard > 0.0 { format!("Carrying {:.0} units of exports out of town", m.exports_aboard) } else { "Empty".to_string() }
                } else {
                    format!("Bringing in {:.0} units: {}", m.import_units(), kinds.iter().take(4).map(|(r, a)| format!("{a:.0} {}", r.label())).collect::<Vec<_>>().join(", "))
                };
                ui.label(egui::RichText::new(text).color(IVORY));
                // [freight] How full the wagons are.
                ui.label(egui::RichText::new(format!("{} wagons  ·  {:.0} of {:.0} units aboard", m.wagons, m.aboard, m.capacity)).small().weak());
            }
        }
    }
    let state = w.rail.state(id);
    status(ui, w, t, state, &mut picked);
    stops(ui, w, t, rail::DWELL_SECONDS, &format!("then out of town to the {to_side}"), &mut picked);
    match t.kind {
        TrainKind::Passenger => passengers(ui, w, &t.passengers, rail::TRAIN_CAPACITY, &mut picked),
        TrainKind::Freight => freight(ui, w, id, &mut picked),
    }
    picked
}

/// Which way a route runs, as compass words for where it begins and ends.
fn ends(t: &Train) -> (&'static str, &'static str) {
    let (Some(a), Some(b)) = (t.poly.first(), t.poly.last()) else { return ("west", "east") };
    let d = *b - *a;
    // The sim's +y is north (the trunk lies south of the centre at negative y).
    let word = |v: nd_sim::Vec2| if v.x.abs() >= v.y.abs() { if v.x >= 0.0 { "east" } else { "west" } } else if v.y >= 0.0 { "north" } else { "south" };
    (word(-d), word(d))
}

fn metro(ui: &mut egui::Ui, w: &World, id: nd_sim::MetroTrainId, t: &nd_sim::MetroTrain) -> Option<Selection> {
    let mut picked = None;
    let line = w.subway.lines.get(t.line);
    let colour = super::theme::rgb_gamma(nd_procgen::metro::line_colour(t.run.color).map(|x| x / 2.0));
    let line_name = line.map_or("A metro line".to_string(), |l| l.name.clone());
    ui.label(egui::RichText::new(format!("{line_name} train {}", metro_code(id))).font(crate::ui::theme::lead()).color(IVORY).strong());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        tag(ui, "Metro", CYAN);
        tag(ui, &line_name, colour);
        tag(ui, &format!("{} cars", nd_sim::subway::CARS), egui::Color32::from_gray(170));
        if line.is_some_and(|l| l.looped) {
            tag(ui, "loop", egui::Color32::from_gray(170));
        }
    });
    if let (Some(a), Some(b)) = (t.run.stops.first(), t.run.stops.last()) {
        let text = if a.0 == b.0 { format!("Round the loop from {}", name_of(w, a.0)) } else { format!("{} → {}  ·  {} stops", name_of(w, a.0), name_of(w, b.0), t.run.stops.len()) };
        ui.label(egui::RichText::new(text).color(CYAN).strong());
    }
    let state = if let Some(at) = t.run.dwelling_at() { TrainState::Dwelling { at, left: t.run.dwell } } else if t.run.next_stop >= t.run.stops.len() { TrainState::Leaving } else { TrainState::Running };
    status(ui, w, &t.run, Some(state), &mut picked);
    stops(ui, w, &t.run, nd_sim::subway::DWELL_SECONDS, "then out of service at the end of the line", &mut picked);
    passengers(ui, w, &t.run.passengers, nd_sim::subway::CAPACITY, &mut picked);
    picked
}

/// The state line: running, standing, held, leaving; speed; on the branch.
fn status(ui: &mut egui::Ui, w: &World, t: &Train, state: Option<TrainState>, picked: &mut Option<Selection>) {
    section(ui, "NOW");
    let moving = matches!(state, Some(TrainState::Running | TrainState::Leaving));
    let kmh = if moving { t.speed * 3.6 } else { 0.0 };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        match state {
            Some(TrainState::Dwelling { at, left }) => {
                ui.label(egui::RichText::new("Standing at").color(GOLD));
                if ui.link(egui::RichText::new(name_of(w, at)).color(GOLD).strong()).on_hover_text("Select the station").clicked() {
                    *picked = Some(Selection::Building(at));
                }
                ui.label(egui::RichText::new(format!("· leaves in {:.0} s", left.max(0.0))).color(GOLD));
            }
            Some(TrainState::HeldBlock { by }) => {
                ui.label(egui::RichText::new("Held at a signal: the block ahead is taken").color(WARN));
                if let Some(o) = by.and_then(|o| w.rail.trains.get(o).map(|x| (o, x))) {
                    ui.label(egui::RichText::new("by").color(WARN));
                    if ui.small_button(format!("train {}", headcode(o.0, o.1.kind))).on_hover_text("Select the train ahead").clicked() {
                        *picked = Some(Selection::Train(o.0));
                    }
                }
            }
            Some(TrainState::HeldBranch) => {
                ui.label(egui::RichText::new("Waiting at the switch: another train is on the single-track branch").color(WARN));
            }
            Some(TrainState::Running) => {
                let next = t.stops.get(t.next_stop).map(|s| name_of(w, s.0)).unwrap_or_default();
                ui.label(egui::RichText::new(format!("Running at {kmh:.0} km/h towards {next}")).color(RUNNING));
            }
            Some(TrainState::Leaving) => {
                ui.label(egui::RichText::new(format!("All calls made: running out at {kmh:.0} km/h")).color(RUNNING));
            }
            None => {}
        }
    });
    if t.on_branch() {
        hint(ui, "on the branch line (single track: one train at a time)");
    }
    let done = rail::path_length(&rail::sub_path(&t.poly, 0.0, t.s));
    let total = t.total().max(1.0);
    ui.add(egui::ProgressBar::new((done / total).clamp(0.0, 1.0)).desired_height(6.0).desired_width(ui.available_width().min(320.0)));
    hint(ui, &format!("{:.1} of {:.1} km run · line speed {:.0} km/h", done / 1000.0, total / 1000.0, t.speed * 3.6));
}

/// Stops made (faint), the one it stands at, and those ahead with ETAs,
/// each a link that selects the station; then where the run ends.
fn stops(ui: &mut egui::Ui, w: &World, t: &Train, dwell: f32, tail: &str, picked: &mut Option<Selection>) {
    let spt = w.config.traffic_seconds_per_tick;
    let calls = t.calls_ahead(dwell, spt, |b| w.buildings.contains_key(b));
    let standing = t.dwelling_at();
    section(ui, &format!("STOPS  ·  {} to go", calls.len()));
    let mut row = |ui: &mut egui::Ui, b: BuildingId, mark: &str, col: egui::Color32, note: String| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(egui::RichText::new(mark).color(col).monospace());
            if ui.link(egui::RichText::new(name_of(w, b)).color(col)).on_hover_text("Select the station").clicked() {
                *picked = Some(Selection::Building(b));
            }
            ui.label(egui::RichText::new(note).small().color(col.gamma_multiply(0.8)));
        });
    };
    for &(b, _) in t.calls_made().iter().filter(|(b, _)| Some(*b) != standing) {
        row(ui, b, "•", egui::Color32::from_gray(120), "called".into());
    }
    if let Some(b) = standing {
        row(ui, b, "●", GOLD, format!("standing here, {:.0} s to go", t.dwell.max(0.0)));
    }
    for (i, c) in calls.iter().enumerate() {
        let next = i == 0;
        let (mark, col) = if next { ("▶", CYAN) } else { ("○", IVORY) };
        let note = if next && standing.is_none() { format!("next stop, {}", eta(w, c.minutes)) } else { eta(w, c.minutes) };
        row(ui, c.stop, mark, col, note);
    }
    let end = t.minutes_to_end(dwell, spt, |b| w.buildings.contains_key(b));
    hint(ui, &format!("{tail} ({})", eta(w, end)));
}

/// Why someone is travelling, in a few words.
fn purpose(p: &nd_sim::citizens::Purpose) -> String {
    use nd_sim::citizens::Purpose as P;
    match p {
        P::Home => "going home".into(),
        P::Work => "going to work".into(),
        P::Visit(lu) => format!("out to the {}", lu.label().to_lowercase()),
        P::Shop(r) => format!("shopping for {}", r.label()),
        P::Wander => "out for a wander".into(),
        P::Sit => "off to sit in a park".into(),
        P::ToStop => "changing to another line".into(),
    }
}

/// Where a rider gets off (rail or metro leg), where they are going in the
/// end and why.
pub fn rider_trip(c: &nd_sim::Citizen) -> (Option<BuildingId>, Option<BuildingId>, Option<String>) {
    let Some(trip) = c.transit else { return (None, None, None) };
    let alight = match trip.leg {
        TransitLeg::Rail { alight, .. } | TransitLeg::Subway { alight, .. } => Some(alight),
        _ => None,
    };
    (alight, Some(trip.target), Some(purpose(&trip.purpose)))
}

/// Everyone aboard: a count against capacity, who gets off where, and a
/// scrolling list of riders (face and name select them).
fn passengers(ui: &mut egui::Ui, w: &World, riders: &[CitizenId], capacity: usize, picked: &mut Option<Selection>) {
    section(ui, &format!("ON BOARD  ·  {} / {}", riders.len(), capacity));
    ui.add(egui::ProgressBar::new(riders.len() as f32 / capacity.max(1) as f32).desired_height(6.0).desired_width(ui.available_width().min(320.0)));
    if riders.is_empty() {
        hint(ui, "nobody aboard just now");
        return;
    }
    // Who gets off where, in call order of first appearance.
    let mut off: Vec<(Option<BuildingId>, usize)> = Vec::new();
    for &p in riders {
        let a = w.citizens.get(p).and_then(|c| rider_trip(c).0);
        match off.iter_mut().find(|(b, _)| *b == a) {
            Some(x) => x.1 += 1,
            None => off.push((a, 1)),
        }
    }
    hint(ui, &format!("getting off: {}", off.iter().map(|(b, n)| format!("{} {n}", b.map_or("?".to_string(), |b| name_of(w, b)))).collect::<Vec<_>>().join(" · ")));
    egui::ScrollArea::vertical().id_salt("train_riders").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
        for &p in riders {
            let Some(c) = w.citizens.get(p) else { continue };
            let (alight, target, why) = rider_trip(c);
            ui.horizontal(|ui| {
                if super::portrait::button(ui, c, 26.0).clicked() {
                    *picked = Some(Selection::Citizen(p));
                }
                ui.vertical(|ui| {
                    if ui.link(egui::RichText::new(&c.name).color(IVORY).strong()).on_hover_text("Select this person").clicked() {
                        *picked = Some(Selection::Citizen(p));
                    }
                    let mut bits = Vec::new();
                    if let Some(a) = alight {
                        bits.push(format!("off at {}", name_of(w, a)));
                    }
                    if let Some(b) = target {
                        bits.push(format!("then to {}", name_of(w, b)));
                    }
                    if let Some(why) = why {
                        bits.push(why);
                    }
                    ui.label(egui::RichText::new(bits.join(" · ")).small().weak());
                });
            });
        }
    });
}

/// What a load of freight is for, from what its buyer does with it.
pub fn load_purpose(w: &World, l: &FreightLoad) -> String {
    let Some(b) = w.buildings.get(l.buyer) else { return "for a buyer that has since closed".into() };
    if b.construction.is_some() {
        return format!("building materials for the site at {}", b.name);
    }
    let Some(bz) = &b.business else { return format!("for {}", b.name) };
    let kind = bz.kind.label().to_lowercase();
    if bz.buys(l.resource) {
        let outs: Vec<&str> = bz.kind.recipe().outputs.iter().map(|(r, _)| r.label()).collect();
        if l.resource.is_retail() || outs.is_empty() {
            format!("stock for the {kind}'s shelves")
        } else {
            format!("material for the {kind} to make {}", outs.join(" and "))
        }
    } else if l.resource.is_retail() || bz.sells(l.resource) {
        format!("goods for the {kind} to sell")
    } else {
        format!("supplies for the {kind}")
    }
}

/// A freight train's loads, by kind and one by one, and its exports.
fn freight(ui: &mut egui::Ui, w: &World, id: nd_sim::TrainId, picked: &mut Option<Selection>) {
    let Some(m) = w.freight_manifest(id) else { return };
    let units = m.import_units();
    section(ui, &format!("FREIGHT  ·  {:.0} units to set down  ·  {:.0} exports aboard", units.max(0.0), m.exports_aboard.max(0.0)));
    // [freight] The wagons and how full they are.
    let fill = if m.capacity > 0.0 { (m.aboard / m.capacity).clamp(0.0, 1.0) } else { 0.0 };
    ui.add(egui::ProgressBar::new(fill).desired_height(10.0).text(egui::RichText::new(format!("{} wagons  ·  {:.0} / {:.0} units", m.wagons, m.aboard, m.capacity)).small()));
    if m.unloaded > 0.0 {
        ui.label(egui::RichText::new(format!("Set down {:.0} units of imports at the yards so far", m.unloaded)).color(RUNNING));
    }
    if m.loads.is_empty() && m.exports_aboard <= 0.0 && m.exports_to_collect.is_empty() {
        hint(ui, if m.unloaded > 0.0 { "nothing more to set down: running back out of town" } else { "running empty: no goods were waiting for its yards when it set off" });
        return;
    }
    if !m.loads.is_empty() {
        // By kind first.
        let mut kinds: Vec<(nd_sim::market::Resource, f32)> = Vec::new();
        for l in &m.loads {
            match kinds.iter_mut().find(|(r, _)| *r == l.resource) {
                Some(k) => k.1 += l.amount,
                None => kinds.push((l.resource, l.amount)),
            }
        }
        let by_kind: Vec<String> = kinds.iter().map(|(r, a)| format!("{a:.0} {}", r.label())).collect();
        ui.label(egui::RichText::new(by_kind.join("  ·  ")).color(GOLD).strong());
        hint(ui, "imports aboard, loaded at the edge of the map");
        egui::ScrollArea::vertical().id_salt("train_freight").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
            for l in &m.loads {
                ui.add_space(2.0);
                ui.label(egui::RichText::new(format!("{:.0} {}  ·  worth {:.0}", l.amount, l.resource.label(), l.price)).color(IVORY).strong());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(egui::RichText::new("aboard from out of town, off at").small().weak());
                    if ui.link(egui::RichText::new(name_of(w, l.yard)).small()).clicked() {
                        *picked = Some(Selection::Building(l.yard));
                    }
                    if let Some(h) = l.hub {
                        ui.label(egui::RichText::new("→ via cargo hub").small().weak());
                        if ui.link(egui::RichText::new(name_of(w, h)).small()).clicked() {
                            *picked = Some(Selection::Building(h));
                        }
                    }
                    ui.label(egui::RichText::new("→ for").small().weak());
                    if ui.link(egui::RichText::new(name_of(w, l.buyer)).small().color(CYAN)).clicked() {
                        *picked = Some(Selection::Building(l.buyer));
                    }
                });
                ui.label(egui::RichText::new(load_purpose(w, l)).small().color(egui::Color32::from_rgb(180, 210, 160)));
            }
        });
    }
    if m.exports_aboard > 0.0 {
        ui.label(format!("{:.0} units of the town's exports aboard, going out to market", m.exports_aboard));
    }
    for (y, a) in &m.exports_to_collect {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("{a:.0} units of exports waiting at")).small());
            if ui.link(egui::RichText::new(name_of(w, *y)).small()).clicked() {
                *picked = Some(Selection::Building(*y));
            }
        });
    }
}
