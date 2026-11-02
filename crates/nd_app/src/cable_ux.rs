//! [cable] Drawing cable car lines station to station (the Buses tool's
//! Cable car mode, Build window, Transit, Cable car), their panel and the
//! station card in the inspector.
//!
//! - **The tool** (`input`): each click is a stop. A click on (or by) a
//!   cable station uses it; anywhere else a station would be built there
//!   (on the roof of a building 14 m or taller, else on the lot, bought
//!   out, else on open ground with a way in). The line so far is laid as
//!   it is drawn: its pylons and the rope sagging between them show over
//!   the city, white, or red with the reason where it cannot be built.
//!   Enter (or the panel's Build) builds it, a right click or Backspace
//!   takes back the last stop, Esc drops the draft; the panel switches
//!   between gondola and aerial tramway.
//! - **The panel**: the kind, where stations go (auto, ground, roof), the
//!   figures (length, pylons, the tallest, ride, capacity, cost), the
//!   refusal; then each line with its riders, waits and capacity used, and
//!   its cabins, hours and fare to set.

use bevy::gizmos::config::{GizmoConfig, GizmoConfigGroup, GizmoLineConfig};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use nd_sim::cable::{self as cable, CableKind, CableLineId, CablePreview, CableStationId};
use nd_sim::{CableStop, SiteWant, Vec2 as SVec2, World};

use crate::camera::Cursor;
use crate::sim::{Sim, SimSet};
use crate::tools::{Tool, ToolState, TransitMode};
use crate::ui::{GOLD, IVORY, WARN};

/// A click this near a cable station picks it.
pub const PICK_RADIUS: f32 = 40.0;

/// The cable line being drawn.
#[derive(Clone, Debug, Default)]
pub struct CableDraft {
    pub kind: CableKind,
    pub stops: Vec<CableStop>,
    /// Where new stations go.
    pub want: SiteWant,
    /// The line so far, laid (recomputed when the stops change).
    pub preview: Option<CablePreview>,
    /// The station last clicked with no line being drawn: its card shows.
    pub picked: Option<CableStationId>,
    /// What a click under the pointer would do, and where it was worked out.
    pub hover: Option<(SVec2, Result<String, String>)>,
}

impl CableDraft {
    fn relay(&mut self, w: &World) {
        self.preview = (self.stops.len() >= 2).then(|| w.cable_line_preview(self.kind, &self.stops));
    }

    fn point(&self, w: &World, k: usize) -> Option<SVec2> {
        match self.stops.get(k)? {
            CableStop::Station(id) => w.cable.stations.get(*id).map(|s| s.pos),
            CableStop::At(p, _) => Some(w.cable.station_near(*p, 30.0).and_then(|id| w.cable.stations.get(id)).map_or(*p, |s| s.pos)),
        }
    }
}

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CableGizmos;

pub struct CableUxPlugin;

impl Plugin for CableUxPlugin {
    fn build(&self, app: &mut App) {
        app.insert_gizmo_config(CableGizmos, GizmoConfig { depth_bias: -0.2, line: GizmoLineConfig { width: 3.0, ..default() }, ..default() })
            .add_systems(Update, cable_gizmos.in_set(SimSet::Input).after(crate::tools::tool_input))
            .add_systems(EguiPrimaryContextPass, panel.after(crate::ui::side_panel));
    }
}

pub fn tool_active(t: &ToolState) -> bool {
    t.tool == Tool::Transit && t.transit_mode == TransitMode::Cable
}

fn kind_label(k: CableKind) -> &'static str {
    match k {
        CableKind::Gondola => "Gondola",
        CableKind::Tramway => "Aerial tramway",
    }
}

/// The Cable car mode's clicks and keys; the status line to show.
#[allow(clippy::too_many_arguments)]
pub fn input(w: &mut World, tools: &mut ToolState, ground: Option<SVec2>, click: bool, undo: bool, finish: bool, switch_kind: bool) {
    let d = &mut tools.cable;
    // Stations bulldozed since: their stops go.
    d.stops.retain(|s| match s {
        CableStop::Station(id) => w.cable.stations.contains_key(*id),
        _ => true,
    });
    if let Some(p) = ground {
        if d.hover.as_ref().is_none_or(|(q, _)| q.distance(p) > 3.0) {
            let verdict = match w.cable_station_at(p, PICK_RADIUS) {
                Some(id) => Ok(format!("{}: use this station", w.cable.stations[id].name)),
                None => w.cable_station_site(p, d.want).map(|s| s.what),
            };
            d.hover = Some((p, verdict));
        }
    }
    if switch_kind {
        d.kind = if d.kind == CableKind::Gondola { CableKind::Tramway } else { CableKind::Gondola };
        d.relay(w);
        tools.status = format!("{}.", kind_label(d.kind));
    }
    if click {
        if let Some(p) = ground {
            let stop = match w.cable_station_at(p, PICK_RADIUS) {
                Some(id) => CableStop::Station(id),
                None => CableStop::At(p, d.want),
            };
            if d.stops.is_empty() {
                if let CableStop::Station(id) = stop {
                    d.picked = Some(id);
                }
            }
            // Refuse a site that cannot take a station at once.
            if let CableStop::At(q, want) = stop {
                if let Err(why) = w.cable_station_site(q, want) {
                    tools.status = format!("No station there: {why}.");
                    return;
                }
            }
            if d.stops.last() == Some(&stop) {
                tools.status = "That is the last stop already: click the next one.".into();
                return;
            }
            d.stops.push(stop);
            d.relay(w);
            tools.status = match (&d.preview, d.stops.len()) {
                (_, 1) => "Line started. Click the next station (an existing one, a roof, a lot or open ground).".into(),
                (Some(p), _) if p.refusal.is_some() => format!("Cannot build it like that: {}. Backspace takes the stop back.", p.refusal.as_deref().unwrap_or("")),
                (Some(p), n) => format!("{n} stops: {:.2} km, {} pylons, {:.0} to build. Click the next stop or Enter to build.", p.length_m / 1000.0, p.pylons, p.build_cost),
                _ => String::new(),
            };
        }
    }
    if undo {
        if d.stops.pop().is_some() {
            d.relay(w);
            tools.status = format!("Took back a stop: {} left.", d.stops.len());
        } else {
            tools.status = "Nothing to take back.".into();
        }
    }
    if finish {
        tools.status = build(w, d);
    }
}

/// Build the draft; the status line.
fn build(w: &mut World, d: &mut CableDraft) -> String {
    if d.stops.len() < 2 {
        return "A line needs two stops: click another.".into();
    }
    match w.add_cable_line(d.kind, &d.stops) {
        Ok(id) => {
            let l = &w.cable.lines[id];
            let msg = format!("{} built: {} stations, {:.2} km, {} pylons, {} cabins. Riders find it by themselves.", l.name, l.stations.len(), l.length() / 1000.0, l.pylons().count(), l.cabins);
            d.stops.clear();
            d.preview = None;
            d.picked = l.stations.first().copied();
            msg
        }
        Err(e) => format!("Not built: {e}."),
    }
}

fn cable_gizmos(sim: Res<Sim>, tools: Res<ToolState>, cursor: Res<Cursor>, time: Res<Time>, mut g: Gizmos<CableGizmos>) {
    if !tool_active(&tools) {
        return;
    }
    let w = &sim.world;
    let pulse = (time.elapsed_secs() * 2.4).sin() * 0.5 + 0.5;
    let ring = |g: &mut Gizmos<CableGizmos>, p: SVec2, z: f32, r: f32, c: Color| {
        g.circle(crate::overlay::shapes::flat_at(Vec3::new(p.x, z + 0.3, p.y)), r, c);
    };
    for (_, s) in w.cable.stations.iter() {
        ring(&mut g, s.pos, s.floor, 18.0, Color::linear_rgb(2.0, 1.4, 0.3));
    }
    let d = &tools.cable;
    let ok = d.preview.as_ref().is_none_or(|p| p.refusal.is_none());
    let colour = if ok { Color::linear_rgb(2.5, 2.5, 2.5) } else { Color::linear_rgb(3.0, 0.3, 0.2) };
    if let Some(p) = d.preview.as_ref().filter(|p| !p.supports.is_empty()) {
        let spec = p.kind.spec();
        for w2 in p.supports.windows(2) {
            let (a, b) = (w2[0], w2[1]);
            let n = ((b.s - a.s) / 8.0).ceil().max(2.0) as usize;
            let pts: Vec<Vec3> = (0..=n)
                .map(|k| {
                    let t = k as f32 / n as f32;
                    let q = a.pos.lerp(b.pos, t);
                    Vec3::new(q.x, cable::rope_z(&p.supports, spec.sag, a.s + (b.s - a.s) * t), q.y)
                })
                .collect();
            g.linestrip(pts, colour);
        }
        for s in &p.supports {
            let base = Vec3::new(s.pos.x, s.base, s.pos.y);
            g.line(base, Vec3::new(s.pos.x, s.top, s.pos.y), if s.station.is_some() { Color::linear_rgb(2.0, 1.4, 0.3) } else { colour });
        }
    } else {
        // Not laid yet (one stop, or refused before any span): straight.
        let pts: Vec<Vec3> = (0..d.stops.len()).filter_map(|k| d.point(w, k)).map(|p| Vec3::new(p.x, w.surface_height(p) + 12.0, p.y)).collect();
        if pts.len() >= 2 {
            g.linestrip(pts, colour);
        }
    }
    for k in 0..d.stops.len() {
        if let Some(p) = d.point(w, k) {
            ring(&mut g, p, w.surface_height(p), 12.0, colour);
        }
    }
    // A lead from the last stop to the pointer, and the pointer's verdict.
    if let (Some(last), Some(c)) = (d.stops.len().checked_sub(1).and_then(|k| d.point(w, k)), cursor.ground) {
        let (a, b) = (Vec3::new(last.x, w.surface_height(last) + 12.0, last.y), Vec3::new(c.x, w.surface_height(c) + 12.0, c.y));
        g.line(a, b, Color::linear_rgb(1.2, 1.2, 1.2).with_alpha(0.5));
    }
    if let Some(c) = cursor.ground {
        let good = d.hover.as_ref().is_some_and(|(_, v)| v.is_ok());
        ring(&mut g, c, w.surface_height(c), 9.0 + 3.0 * pulse, if good { Color::linear_rgb(0.4, 2.2, 0.8) } else { Color::linear_rgb(2.5, 0.4, 0.3) });
    }
}

fn panel(mut contexts: EguiContexts, mut sim: ResMut<Sim>, mut tools: ResMut<ToolState>) -> Result {
    if !tool_active(&tools) {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let tools = &mut *tools;
    let w = &mut sim.world;
    let mut act: Option<&str> = None;
    let mut set_kind: Option<CableKind> = None;
    let mut set_want: Option<SiteWant> = None;
    let mut edits: Vec<(CableLineId, LineEdit)> = Vec::new();
    egui::Window::new("Cable car line")
        .id(egui::Id::new("cable_line_panel"))
        .default_pos(egui::pos2(12.0, 64.0))
        .default_width(330.0)
        .resizable(false)
        .collapsible(true)
        .show(ctx, |ui| {
            ui.set_max_width(330.0);
            let d = &tools.cable;
            ui.horizontal(|ui| {
                for k in CableKind::ALL {
                    if ui.selectable_label(d.kind == k, kind_label(k)).clicked() {
                        set_kind = Some(k);
                    }
                }
            });
            let spec = d.kind.spec();
            ui.label(egui::RichText::new(match d.kind {
                CableKind::Gondola => format!("A loop of {}-seat cabins: board any passing one. Spans to {:.0} m, pylons to {:.0} m; may turn at stations. Fare {:.1}.", spec.seats, spec.max_span, spec.max_pylon, spec.fare),
                CableKind::Tramway => format!("Two {}-seat cabins shuttling between two stations. Spans to {:.1} km (across a lake, up a cliff), pylons to {:.0} m. Fare {:.1}.", spec.seats, spec.max_span / 1000.0, spec.max_pylon, spec.fare),
            }).small().color(IVORY));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("new stations:").small());
                for (wv, label) in [(SiteWant::Auto, "auto"), (SiteWant::Ground, "ground"), (SiteWant::Roof, "roof")] {
                    if ui.selectable_label(d.want == wv, label).clicked() {
                        set_want = Some(wv);
                    }
                }
            });
            if let Some((_, v)) = &d.hover {
                match v {
                    Ok(what) => ui.label(egui::RichText::new(format!("here: {what}")).small().color(GOLD)),
                    Err(why) => ui.label(egui::RichText::new(format!("here: {why}")).small().color(WARN)),
                };
            }
            let n = d.stops.len();
            let steps: [(&str, bool, bool); 3] = [
                ("1. Click where the line starts (a station, a roof, a lot, a hillside).", n >= 1, n == 0),
                ("2. Click the next stop: the pylons and rope are laid as you go.", n >= 2, n == 1),
                ("3. More stops (a gondola turns at stations), then Enter or Build.", false, n >= 2),
            ];
            for (text, done, now) in steps {
                let t = egui::RichText::new(if done { format!("✔ {text}") } else { text.to_string() }).small();
                ui.label(if now { t.color(GOLD).strong() } else if done { t.weak() } else { t.color(IVORY) });
            }
            if let Some(p) = &d.preview {
                egui::Grid::new("cable_preview").num_columns(2).spacing(egui::vec2(10.0, 1.0)).show(ui, |ui| {
                    let mut r = |a: &str, b: String| {
                        ui.label(egui::RichText::new(a).small().weak());
                        ui.label(egui::RichText::new(b).small());
                        ui.end_row();
                    };
                    r("length", format!("{:.2} km, {} stations ({} new)", p.length_m / 1000.0, p.stations, p.new_stations));
                    r("pylons", format!("{} (tallest {:.0} m)", p.pylons, p.tallest_pylon_m));
                    r("highest", format!("{:.0} m over the ground", p.max_above_ground_m));
                    r("ride", format!("{:.0} min end to end, a cabin every {:.1} min", p.ride_min, p.interval_min));
                    r("capacity", format!("{:.0} an hour each way ({} cabins)", p.capacity_per_hour, p.cabins));
                    r("cost", format!("{:.0} to build, {:.0}/day", p.build_cost, p.upkeep_per_day));
                    r("view", format!("{:.0}% scenic{}", p.scenic * 100.0, if p.scenic >= 0.2 { " (draws visitors)" } else { "" }));
                });
                if let Some(why) = &p.refusal {
                    ui.label(egui::RichText::new(format!("Cannot build: {why}")).small().color(WARN));
                }
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(n >= 2 && d.preview.as_ref().is_some_and(|p| p.refusal.is_none()), egui::Button::new("Build")).clicked() {
                    act = Some("build");
                }
                if ui.add_enabled(n > 0, egui::Button::new("Undo stop")).clicked() {
                    act = Some("undo");
                }
                if ui.add_enabled(n > 0, egui::Button::new("Cancel")).clicked() {
                    act = Some("cancel");
                }
            });
            ui.label(egui::RichText::new("Enter builds · right click / Backspace undoes · Esc cancels").small().weak());
            if !w.cable.lines.is_empty() {
                ui.separator();
                let ids: Vec<CableLineId> = w.cable.lines.keys().collect();
                for id in ids {
                    line_card(ui, w, id, &mut edits);
                }
            }
            if let Some(s) = d.picked.filter(|s| w.cable.stations.contains_key(*s)) {
                ui.separator();
                station_card(ui, w, s);
            }
        });
    let d = &mut tools.cable;
    if let Some(k) = set_kind {
        d.kind = k;
        d.relay(w);
    }
    if let Some(wv) = set_want {
        d.want = wv;
        for s in &mut d.stops {
            if let CableStop::At(p, _) = *s {
                *s = CableStop::At(p, wv);
            }
        }
        d.hover = None;
        d.relay(w);
    }
    match act {
        Some("build") => tools.status = build(w, d),
        Some("undo") => {
            d.stops.pop();
            d.relay(w);
        }
        Some("cancel") => {
            d.stops.clear();
            d.preview = None;
            tools.status = "Line cancelled: nothing was built.".into();
        }
        _ => {}
    }
    for (id, e) in edits {
        let r = match e {
            LineEdit::Cabins(n) => w.set_cable_cabins(id, n).map(|_| ()),
            LineEdit::Hours(o, c) => w.set_cable_hours(id, o, c).map(|_| ()),
            LineEdit::Fare(f) => w.set_cable_fare(id, f).map(|_| ()),
            LineEdit::Remove => {
                w.remove_cable_line(id);
                Ok(())
            }
        };
        if let Err(e) = r {
            tools.status = e;
        }
    }
    Ok(())
}

enum LineEdit {
    Cabins(u32),
    Hours(u32, u32),
    Fare(f32),
    Remove,
}

/// One line's card: riders, waits, capacity used; its cabins, hours and fare.
fn line_card(ui: &mut egui::Ui, w: &World, id: CableLineId, edits: &mut Vec<(CableLineId, LineEdit)>) {
    let Some(l) = w.cable.lines.get(id) else { return };
    let secs = w.config.traffic_seconds_per_tick;
    let colour = nd_procgen::metro::line_colour(l.color);
    let c = egui::Color32::from(egui::Rgba::from_rgb((colour[0] * 1.5).min(1.0), (colour[1] * 1.5).min(1.0), (colour[2] * 1.5).min(1.0)));
    let names: Vec<String> = l.stations.iter().filter_map(|s| w.cable.stations.get(*s).map(|x| x.name.clone())).collect();
    ui.label(egui::RichText::new(format!("{} · {}", l.name, kind_label(l.kind))).color(c).strong());
    ui.label(egui::RichText::new(names.join(" > ")).small());
    let waiting: u32 = l.stations.iter().map(|s| w.cable_station_waiting(*s)).sum();
    let (_, wait_y, _, _) = l.timeline.totals(true);
    let (_, wait_t, _, _) = l.timeline.totals(false);
    let wait = wait_y.or(wait_t);
    ui.label(egui::RichText::new(format!(
        "{} riders today ({} yesterday) · {} waiting · {} aboard · mean wait {} · capacity used {:.0}%{}",
        l.riders_today,
        l.riders_yesterday,
        waiting,
        l.aboard.len(),
        wait.map_or("-".to_string(), |m| format!("{m:.1} min")),
        l.capacity_used() * 100.0,
        if l.left_behind_today > 0 { format!(" · full cabins left {} behind", l.left_behind_today) } else { String::new() }
    )).small());
    ui.label(egui::RichText::new(format!(
        "{:.2} km · {} pylons · a cabin every {:.1} min · {:.0}/h each way · {:.0}/day · {}{}",
        l.length() / 1000.0,
        l.pylons().count(),
        l.interval(secs),
        l.capacity_per_hour(secs),
        l.upkeep_per_day(),
        if l.open_at(w.clock.minute_of_day() as u32) { "open" } else { "closed" },
        if l.sightseers_today + l.sightseers_yesterday > 0 { format!(" · {} sightseers today", l.sightseers_today) } else { String::new() }
    )).small().weak());
    ui.horizontal(|ui| {
        if l.kind == CableKind::Gondola {
            let (lo, hi) = cable::cabin_limits(l.kind, l.loop_layout().length);
            let mut n = l.cabins;
            if ui.add(egui::Slider::new(&mut n, lo..=hi).text("cabins")).changed() {
                edits.push((id, LineEdit::Cabins(n)));
            }
        }
        let mut f = l.fare;
        if ui.add(egui::DragValue::new(&mut f).range(0.0..=cable::MAX_FARE).speed(0.1).prefix("fare ")).changed() {
            edits.push((id, LineEdit::Fare(f)));
        }
    });
    ui.horizontal(|ui| {
        let (mut o, mut c) = (l.open / 60, l.close / 60);
        let a = ui.add(egui::DragValue::new(&mut o).range(0..=23).prefix("opens ").suffix(":00")).changed();
        let b = ui.add(egui::DragValue::new(&mut c).range(0..=24).prefix("closes ").suffix(":00")).changed();
        if (a || b) && o * 60 != (c * 60) % 1440 {
            edits.push((id, LineEdit::Hours(o * 60, (c * 60) % 1440)));
        }
        if ui.small_button("Close the line").clicked() {
            edits.push((id, LineEdit::Remove));
        }
    });
}

/// A station's card: where it stands, the lines calling, boardings, the queue.
pub fn station_card(ui: &mut egui::Ui, w: &World, s: CableStationId) {
    let Some(st) = w.cable.stations.get(s) else { return };
    ui.label(egui::RichText::new(format!("{} (cable station)", st.name)).color(GOLD).strong());
    let site = if st.roof {
        format!("on the roof of {}, {:.0} m above the street", st.building.and_then(|b| w.buildings.get(b)).map_or("a building".to_string(), |b| b.name.clone()), st.floor - st.ground)
    } else if st.building.is_some() {
        "a ground station on its own lot".to_string()
    } else {
        "on open ground".to_string()
    };
    ui.label(egui::RichText::new(site).small());
    let lines: Vec<String> = w.cable.lines_at(s).map(|(_, l)| format!("{} ({})", l.name, l.kind.label())).collect();
    ui.label(egui::RichText::new(format!(
        "{} · {} boarded today, {} yesterday · {} waiting",
        if lines.is_empty() { "no line calls here".to_string() } else { lines.join(", ") },
        st.boardings_today,
        st.boardings_yesterday,
        w.cable_station_waiting(s)
    )).small());
}

/// [cable] The inspector's section for a building that is (or carries) a
/// cable car station.
pub fn building_card(ui: &mut egui::Ui, w: &World, b: nd_sim::BuildingId) {
    let ids: Vec<CableStationId> = w.cable.stations.iter().filter(|(_, s)| s.building == Some(b)).map(|(id, _)| id).collect();
    for s in ids {
        station_card(ui, w, s);
        let lines: Vec<CableLineId> = w.cable.lines_at(s).map(|(id, _)| id).collect();
        let secs = w.config.traffic_seconds_per_tick;
        for id in lines {
            let l = &w.cable.lines[id];
            let (_, wait, _, _) = l.timeline.totals(true);
            ui.label(egui::RichText::new(format!(
                "{}: {} riders today, {} yesterday · a cabin every {:.1} min · mean wait {} · capacity used {:.0}% · {}–{}",
                l.name,
                l.riders_today,
                l.riders_yesterday,
                l.interval(secs),
                wait.map_or("-".to_string(), |m| format!("{m:.1} min")),
                l.capacity_used() * 100.0,
                nd_sim::timetable::hm(l.open),
                nd_sim::timetable::hm(l.close)
            )).small());
        }
    }
}
