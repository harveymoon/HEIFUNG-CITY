//! [events] Street races, raves and the roads in the HUD: news lines as
//! toasts and in City Data's Services, Emergencies, a racer's line in the car
//! inspector, a rave's in the warehouse's, and the road maintenance budget
//! in City Data's Economy, Taxes.

use bevy::prelude::Vec2;
use bevy_egui::egui;
use nd_sim::road_wear;
use nd_sim::street_events::EventKind;

use super::pane::heading;
use super::{hint, UiCtx, UiFlags, GOLD, WARN};

/// Neon pink for raves, electric blue for races.
pub const RAVE: egui::Color32 = egui::Color32::from_rgb(255, 90, 220);
pub const RACE: egui::Color32 = egui::Color32::from_rgb(90, 200, 255);
/// [moto] Hot orange for moto packs.
pub const MOTO: egui::Color32 = egui::Color32::from_rgb(255, 140, 40);

fn kind_color(k: EventKind) -> egui::Color32 {
    match k {
        EventKind::Race => RACE,
        EventKind::Rave => RAVE,
        EventKind::MotoPack => MOTO,
    }
}

/// Fresh city news shows as the toast (once, and only while it is news:
/// a load does not replay last night's).
pub fn news_toast(w: &nd_sim::World, flags: &mut UiFlags) {
    let Some(n) = w.street_events.news.last() else { return };
    if n.seq <= flags.news_seen {
        return;
    }
    flags.news_seen = n.seq;
    if w.clock.tick.saturating_sub(n.tick) <= 30 {
        flags.message = n.headline.clone();
    }
}

/// The night's news, newest first, with a button to fly there.
pub fn news(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let e = &w.street_events;
    heading(ui, "Street nights");
    let now = match (&e.race, &e.rave) {
        (Some(r), _) => Some(format!("Race {} · {} cars · {} to {}", r.stage.label(), r.cars.len(), r.meet.name, r.finish.name)),
        (None, Some(r)) => Some(format!("Rave at {} · {} inside · {} queueing", r.place, r.inside.len(), r.queue.len())),
        _ => None,
    };
    if let Some(t) = now {
        ui.label(egui::RichText::new(t).color(if e.race.is_some() { RACE } else { RAVE }));
    }
    hint(ui, &format!("{} races · {} raves · {} busted · {:.0} in fines", e.races_total, e.raves_total, e.busts_total, e.fines_total));
    if e.news.is_empty() {
        hint(ui, "Quiet nights so far.");
        return;
    }
    let mut fly = None;
    for n in e.news.iter().rev().take(6) {
        ui.horizontal(|ui| {
            if ui.small_button("go").clicked() {
                fly = Some(n.pos);
            }
            ui.label(egui::RichText::new(format!("{} · {}", super::emergencies::when(n.tick), n.headline)).small().color(kind_color(n.kind)));
        });
    }
    if let Some(at) = fly {
        p.fly_to = Some(Vec2::new(at.x, at.y));
    }
}

/// A street racer's car: where the race is and how they are placed.
pub fn racer_line(ui: &mut egui::Ui, w: &nd_sim::World, car: nd_sim::CarId) {
    let Some(r) = w.street_events.race_of_car(car) else { return };
    let what = match r.place_of(car) {
        Some(1) => "won the race".to_string(),
        Some(k) => format!("came in {k} of {}", r.cars.len()),
        None => match r.stage {
            nd_sim::street_events::RaceStage::Gathering => format!("gathering at {}", r.meet.name),
            _ => format!("racing to {}", r.finish.name),
        },
    };
    ui.label(egui::RichText::new(format!("STREET RACER · {what}")).color(RACE).strong());
    hint(ui, &format!("{} to {} · {:.1} km · {} cars{}", r.meet.name, r.finish.name, r.route_m / 1000.0, r.cars.len(), if r.incident.is_some() { " · someone called the police" } else { "" }));
}

/// [moto] A rider in a moto pack: where the pack is going.
pub fn moto_line(ui: &mut egui::Ui, w: &nd_sim::World, car: nd_sim::CarId) {
    let Some(p) = w.street_events.pack_of_car(car) else { return };
    ui.label(egui::RichText::new(format!("MOTO PACK · {} riders through {}", p.riders.len(), p.via_name)).color(MOTO).strong());
    hint(ui, &format!("met at {} · {} reds run · {} lane changes{}", p.place, p.red_runs, p.lane_changes, if p.incident.is_some() { " · someone called the police" } else { "" }));
}

/// A warehouse with a rave on (or the scene round it).
pub fn rave_line(ui: &mut egui::Ui, w: &nd_sim::World, b: nd_sim::BuildingId) {
    if let Some(r) = w.street_events.rave.as_ref().filter(|r| r.building == b) {
        ui.label(egui::RichText::new(format!("RAVE TONIGHT · {} inside · {} queueing", r.inside.len(), r.queue.len())).color(RAVE).strong());
        let police = if r.incident.is_some() { " · the police have been called" } else { "" };
        hint(ui, &format!("{} complaints from the neighbours{police} · {}", r.complaints, r.pull.why()));
    } else if let Some(s) = w.street_events.rave_spots.iter().find(|s| s.building == b && s.pull.has_scene()) {
        hint(ui, &format!("Old warehouse with a scene round it: {}", s.pull.why()));
    }
}

/// The road maintenance line: funding slider, what it buys, who it serves.
pub fn road_budget(ui: &mut egui::Ui, p: &mut UiCtx) {
    heading(ui, "Road maintenance");
    let w = &p.sim.world;
    let rw = &w.road_wear;
    let need = rw.need_per_day(&w.roads);
    let mut pct = rw.funding * 100.0;
    let mean = rw.mean_wear(&w.roads);
    let by = w.road_wear_by_land_value();
    let waiting = w.repair_queue().len();
    let last = rw.days.last().cloned();
    ui.horizontal(|ui| {
        ui.spacing_mut().slider_width = 160.0;
        if ui
            .add(egui::Slider::new(&mut pct, 0.0..=road_wear::MAX_FUNDING * 100.0).step_by(5.0).suffix("%"))
            .on_hover_text("Share of what the street network needs to stay as it is. Crews resurface pieces worn past 30%, dearest land first: cut the line and the poor streets go first.")
            .changed()
        {
            p.sim.world.set_road_funding(pct / 100.0);
        }
        ui.label(egui::RichText::new(format!("{:.0}/day allowed of {need:.0} needed", need * pct / 100.0)).small().weak());
    });
    let cond = egui::RichText::new(format!("streets {:.0}% good on average · {waiting} pieces waiting for crews", (1.0 - mean) * 100.0)).small();
    ui.label(if mean > 0.45 { cond.color(WARN) } else { cond });
    ui.label(egui::RichText::new(format!("wear by land: cheap {:.0}% · middle {:.0}% · dear {:.0}%", by[0].0 * 100.0, by[1].0 * 100.0, by[2].0 * 100.0)).small().color(GOLD));
    if let Some(d) = last {
        hint(ui, &format!("last night: {:.0} spent, {} pieces resurfaced (land value {:.2})", d.spent, d.repaired, d.repaired_land_value));
    }
}

/// The street piece under the pointer: name, condition and what it does
/// to traffic, and where it stands in the crews' queue.
pub fn road_card(ctx: &egui::Context, w: &nd_sim::World, ground: nd_sim::Vec2, pointer: egui::Pos2) {
    let Some((sid, _, q)) = w.roads.nearest_on_network(ground) else { return };
    let Some(seg) = w.roads.try_segment(sid) else { return };
    let half = (seg.lanes_fwd + seg.lanes_bwd).max(1) as f32 * nd_sim::roads::LANE_WIDTH * 0.5 + 1.0;
    if q.distance(ground) > half || !road_wear::wears(&w.roads, sid.0 as usize) {
        return;
    }
    let wear = w.road_wear_of(sid);
    let rough = road_wear::roughness(wear);
    let name = w.streets.of(sid).map(|s| s.name.clone()).unwrap_or_else(|| "Street".to_string());
    let lv = w.land_value.at(q);
    let queue = w.repair_queue();
    let place = queue.iter().position(|j| j.segment == sid);
    let col = if wear >= road_wear::CRUMBLING { WARN } else if wear >= road_wear::WORN { egui::Color32::from_rgb(240, 190, 90) } else { egui::Color32::from_rgb(140, 220, 150) };
    egui::Area::new(egui::Id::new("road_hover_card")).order(egui::Order::Tooltip).interactable(false).fixed_pos(pointer + egui::vec2(18.0, 18.0)).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_max_width(280.0);
            super::neon(ui, &name);
            ui.label(egui::RichText::new(format!("surface {:.0}% · {}{}", (1.0 - wear) * 100.0, road_wear::label(wear), if seg.bridge { " · bridge" } else { "" })).small().color(col));
            if rough > 0.0 {
                ui.label(egui::RichText::new(format!("cars {:.0}% slower · crashes x{:.1}", (1.0 - road_wear::slow_factor(rough)) * 100.0, road_wear::crash_factor(rough))).small());
            }
            let queued = match place {
                Some(k) => format!("crews' queue: {} of {}", k + 1, queue.len()),
                None if wear >= road_wear::REPAIR_AT => "waiting for the crews".to_string(),
                None => "not due for repair".to_string(),
            };
            ui.label(egui::RichText::new(format!("{queued} · land value {lv:.2}")).small().weak());
        });
    });
}
