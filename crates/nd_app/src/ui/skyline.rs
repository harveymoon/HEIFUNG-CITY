//! [supertall] The skyline in the HUD: the inspector's SKYLINE line (how
//! tall a building stands, its place in the city, and for a league tower
//! its class, form, developer and the plots it was assembled from), City
//! Data's City, Skyline page (the city's height limit, the tallest
//! buildings, the towers rising, the record book and the news) and the
//! news toast.

use bevy::prelude::Vec2;
use bevy_egui::egui;
use nd_sim::towers::{ordinal, League, MEGATALL_M, SKYSCRAPER_M, SUPERTALL_M};

use super::pane::{self, heading, row};
use super::{hint, section, UiCtx, UiFlags, GOLD, IVORY};
use crate::tools::Selection;

/// Steel blue for skyscrapers, violet for supertalls, hot pink for
/// megatalls.
pub fn league_color(l: League) -> egui::Color32 {
    match l {
        League::Skyscraper => egui::Color32::from_rgb(120, 190, 255),
        League::Supertall => egui::Color32::from_rgb(190, 130, 255),
        League::Megatall => egui::Color32::from_rgb(255, 90, 200),
    }
}

fn money(v: f64) -> String {
    if v >= 1e9 {
        format!("${:.1}B", v / 1e9)
    } else {
        format!("${:.0}M", v / 1e6)
    }
}

/// The inspector's line for any standing building; a league tower's
/// record in full.
pub fn building_section(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId, selection: &mut Selection) {
    let Some(b) = w.buildings.get(id) else { return };
    if b.land_use == nd_sim::LandUse::Park || b.is_power() || b.is_parking() {
        return;
    }
    let h = w.building_height(id);
    let Some(t) = b.plot.tower else {
        if let Some(r) = w.skyline_rank(id).filter(|r| *r <= 10) {
            hint(ui, &format!("about {h:.0} m high · the city's {} tallest", if r == 1 { String::new() } else { ordinal(r) }).replace("  ", " "));
        }
        return;
    };
    section(ui, "SKYLINE");
    let league = t.league();
    let rank = w.skyline_rank(id);
    let place = match rank {
        Some(1) => "the city's tallest".to_string(),
        Some(r) => format!("the city's {} tallest", ordinal(r)),
        None => "rising".to_string(),
    };
    ui.label(egui::RichText::new(format!("{:.0} m {} · {} · {}", t.height, league.label(), t.design.label(), place)).strong().color(league_color(league)));
    let dev = w.owner_of(id).map_or("a developer".to_string(), |(_, o)| o.name.clone());
    let lots = 1 + b.plot.extra_lots.len() + b.plot.pending_lots.len();
    let area = w.footprint(id).map_or(0.0, |l| l.area());
    let mut bits = vec![format!("built by {dev}"), format!("{lots} plot{} assembled ({area:.0} m²)", if lots == 1 { "" } else { "s" })];
    if !b.plot.pending_lots.is_empty() {
        bits.push(format!("{} still being cleared", b.plot.pending_lots.len()));
    }
    match (&b.construction, t.topped_out_day) {
        (Some(c), _) => bits.push(format!("{:.0}% built, about {:.0} days to go", c.progress * 100.0, c.days_left())),
        (None, Some(d)) => bits.push(format!("topped out on day {d}")),
        _ => {}
    }
    if t.record_bid {
        bits.push("planned as a record".into());
    }
    hint(ui, &bits.join(" · "));
    // The towers it outranks or trails, a click away.
    if let Some(r) = rank {
        let ranking = w.skyline_ranking();
        ui.horizontal_wrapped(|ui| {
            for (other, oh) in ranking.iter().skip(r.saturating_sub(2)).take(3) {
                if *other == id {
                    continue;
                }
                let name = &w.buildings[*other].name;
                if ui.small_button(format!("{name} {oh:.0} m")).clicked() {
                    *selection = Selection::Building(*other);
                }
            }
        });
    }
}

/// City Data, City, Skyline.
pub fn page(ui: &mut egui::Ui, p: &mut UiCtx) {
    let mut fly: Option<Vec2> = None;
    let mut pick: Option<nd_sim::BuildingId> = None;
    {
        let w = &p.sim.world;
        let towers = w.league_towers();
        let count = |l: League| towers.iter().filter(|id| w.buildings[**id].construction.is_none() && w.buildings[**id].plot.tower.is_some_and(|t| t.league() == l)).count();
        pane::cards(ui, |ui| {
            let tallest = w.tallest_standing();
            pane::kpi(ui, "tallest", egui::RichText::new(tallest.map_or("-".to_string(), |(_, h)| format!("{h:.0} m"))).color(GOLD));
            pane::kpi(ui, "skyscrapers (150 m+)", egui::RichText::new(format!("{}", count(League::Skyscraper))).color(league_color(League::Skyscraper)));
            pane::kpi(ui, "supertalls (300 m+)", egui::RichText::new(format!("{}", count(League::Supertall))).color(league_color(League::Supertall)));
            pane::kpi(ui, "megatalls (600 m+)", egui::RichText::new(format!("{}", count(League::Megatall))).color(league_color(League::Megatall)));
            pane::kpi(ui, "rising", egui::RichText::new(format!("{}", towers.iter().filter(|id| w.buildings[**id].construction.is_some()).count())));
        });
        heading(ui, "Height limit");
        hint(ui, "Developers compete for the city's tallest: a new tower aims over the record when demand and their money allow. A limit stops the race where you set it (standing towers stay). Districts can set their own (District tool, 9).");
    }
    // The limit: editable.
    {
        let w = &mut p.sim.world;
        let mut on = w.skyline.limit.is_some();
        let mut m = w.skyline.limit.unwrap_or(450.0);
        ui.horizontal(|ui| {
            if ui.checkbox(&mut on, "city-wide limit").changed() {
                w.set_height_limit(on.then_some(m));
            }
            if on && ui.add(egui::Slider::new(&mut m, 30.0..=1000.0).suffix(" m").step_by(10.0)).changed() {
                w.set_height_limit(Some(m));
            }
            for (label, v) in [("no skyscrapers", 120.0), ("skyscrapers", SUPERTALL_M - 10.0), ("supertalls", MEGATALL_M - 10.0)] {
                if ui.small_button(label).on_hover_text(format!("limit {v:.0} m")).clicked() {
                    w.set_height_limit(Some(v));
                }
            }
            if ui.small_button("no limit").clicked() {
                w.set_height_limit(None);
            }
        });
        let mut clear = w.skyline.vertiport_clearance;
        if ui.checkbox(&mut clear, "keep the vertiports' approach clear").on_hover_text("Nothing over 120 m within 350 m of a vertiport you placed").changed() {
            w.skyline.vertiport_clearance = clear;
        }
    }
    let w = &p.sim.world;
    let s = &w.skyline;
    let pop = w.citizens.len();
    row(ui, "the city can carry", egui::RichText::new(format!("{:.0} m at {pop} people", nd_sim::towers::height_for_population(pop))));
    row(ui, "tallest standing or rising", egui::RichText::new(format!("{:.0} m", w.tallest_planned())));
    row(ui, "demand for tower space", egui::RichText::new(format!("{:.2}", w.tower_demand())));
    if pop < nd_sim::towers::TOWER_MIN_POP && !s.always {
        hint(ui, &format!("Nobody builds in the league before the city has {} people.", nd_sim::towers::TOWER_MIN_POP));
    }

    heading(ui, "The tallest buildings");
    let ranking = w.skyline_ranking();
    pane::table(ui, "skyline_rank", &["#", "building", "height", "class", "developer", ""], |ui| {
        for (i, (id, h)) in ranking.iter().take(15).enumerate() {
            let b = &w.buildings[*id];
            ui.label(egui::RichText::new(format!("{}", i + 1)).color(if i == 0 { GOLD } else { IVORY }));
            if ui.link(&b.name).clicked() {
                pick = Some(*id);
            }
            ui.label(format!("{h:.0} m"));
            match b.plot.tower {
                Some(t) => ui.label(egui::RichText::new(t.league().label()).color(league_color(t.league()))),
                None => ui.label(egui::RichText::new(format!("{} floors", b.floors)).weak()),
            };
            ui.label(w.owner_of(*id).map_or(String::new(), |(_, o)| o.name.clone()));
            if ui.small_button("go").clicked() {
                fly = w.footprint(*id).map(|l| Vec2::new(l.center.x, l.center.y));
            }
            ui.end_row();
        }
    });

    let rising: Vec<nd_sim::BuildingId> = w.league_towers().into_iter().filter(|id| w.buildings[*id].construction.is_some()).collect();
    if !rising.is_empty() {
        heading(ui, "Rising");
        pane::table(ui, "skyline_rising", &["tower", "height", "built", "plots", "developer", ""], |ui| {
            for id in &rising {
                let b = &w.buildings[*id];
                let t = b.plot.tower.unwrap();
                if ui.link(&b.name).clicked() {
                    pick = Some(*id);
                }
                ui.label(egui::RichText::new(format!("{:.0} m {}", t.height, t.league().label())).color(league_color(t.league())));
                ui.label(format!("{:.0}%", b.construction.as_ref().map_or(1.0, |c| c.progress) * 100.0));
                ui.label(format!("{}", 1 + b.plot.extra_lots.len() + b.plot.pending_lots.len()));
                ui.label(w.owner_of(*id).map_or(String::new(), |(_, o)| o.name.clone()));
                if ui.small_button("go").clicked() {
                    fly = w.footprint(*id).map(|l| Vec2::new(l.center.x, l.center.y));
                }
                ui.end_row();
            }
        });
    }

    heading(ui, "The record");
    if s.records.is_empty() {
        hint(ui, &format!("No tower of {SKYSCRAPER_M:.0} m or more yet."));
    }
    for r in s.records.iter().rev().take(8) {
        ui.label(egui::RichText::new(format!("day {} · {} by {} · {:.0} m", r.day, r.name, r.developer, r.height)).small().color(if std::ptr::eq(r, s.records.last().unwrap()) { GOLD } else { IVORY }));
    }

    heading(ui, "Developers");
    for d in &s.developers {
        row(ui, &d.name, egui::RichText::new(format!("{} to build with", money(d.capital))));
    }
    if s.developers.is_empty() {
        hint(ui, "No tower developer in town yet.");
    }

    heading(ui, "News");
    for n in s.news.iter().rev().take(8) {
        ui.horizontal(|ui| {
            if ui.small_button("go").clicked() {
                fly = Some(Vec2::new(n.pos.x, n.pos.y));
            }
            ui.label(egui::RichText::new(format!("{} · {}", super::emergencies::when(n.tick), n.headline)).small());
        });
    }
    if let Some(id) = pick {
        *p.selection = Selection::Building(id);
    }
    if let Some(at) = fly {
        p.fly_to = Some(at);
    }
}

/// Fresh skyline news as the toast (once, and only while it is news).
pub fn news_toast(w: &nd_sim::World, flags: &mut UiFlags) {
    let Some(n) = w.skyline.news.last() else { return };
    if n.seq <= flags.skyline_seen {
        return;
    }
    flags.skyline_seen = n.seq;
    if w.clock.tick.saturating_sub(n.tick) <= 30 {
        flags.message = n.headline.clone();
    }
}
