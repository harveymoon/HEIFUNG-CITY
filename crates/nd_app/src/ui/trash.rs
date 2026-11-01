//! [r5-trash] Where the trash goes, for the player: City Data, Services,
//! Trash (tonnes collected, where they went and what each way cost, every
//! disposal site and depot yard, the starter contract) and the inspector's
//! lines for a disposal site or a depot (`nd_sim::waste`).

use bevy_egui::egui;
use nd_sim::waste::{Dest, YARD_T};
use nd_sim::{BuildingId, ServiceKind, World};

use super::pane::{cards, heading, kpi};
use super::{hint, UiCtx, GOLD, IVORY, WARN};

fn t(x: f32) -> String {
    if x >= 100.0 {
        format!("{x:.0} t")
    } else {
        format!("{x:.1} t")
    }
}

/// The inspector's lines for a disposal site or a trash depot.
pub fn inspector_lines(ui: &mut egui::Ui, w: &World, id: BuildingId, kind: ServiceKind) {
    let d = &w.society.trash.disposal;
    if kind == ServiceKind::TrashDepot {
        let held = d.yards.get(&id).copied().unwrap_or(0.0);
        let tier = w.buildings.get(id).and_then(|b| b.service).map_or(nd_sim::Tier::Mid, |s| s.tier);
        let at = w.building_lot(id).map(|l| l.center).unwrap_or_default();
        let sites: Vec<String> = w
            .waste_sites()
            .into_iter()
            .filter(|s| s.pos.distance(at) <= nd_sim::waste::DUMP_REACH)
            .map(|s| format!("{} ({:.1} km)", w.buildings.get(s.id).map_or("?", |b| b.name.as_str()), s.pos.distance(at) / 1000.0))
            .collect();
        let starter = d.starter_on(w.clock.day());
        let yard = if starter { format!("Yard: {} waiting for the contractor (no limit on the starter contract)", t(held)) } else { format!("Yard: {} of {} waiting for the contractor", t(held), t(YARD_T[tier.index()])) };
        ui.label(egui::RichText::new(yard).color(if !starter && held >= YARD_T[tier.index()] - 0.5 { WARN } else { IVORY }).small());
        if sites.is_empty() {
            hint(ui, &format!("No landfill, incinerator, recycling plant or transfer station within {:.0} km: every load goes out of town by lorry at {:.0} a tonne.", nd_sim::waste::DUMP_REACH / 1000.0, w.road_price()));
        } else {
            hint(ui, &format!("Its trucks tip at {}.", sites.join(", ")));
        }
        return;
    }
    let Some(st) = d.sites.get(&id) else {
        hint(ui, "Not taking trash yet.");
        return;
    };
    let Some(info) = w.waste_sites().into_iter().find(|s| s.id == id) else { return };
    let cap = nd_sim::waste::site_capacity(kind, info.tier);
    let line = match kind {
        ServiceKind::Landfill => format!("{} of {} in the ground ({:.0}% full) · {} tipped yesterday", t(st.filled_t), t(cap), 100.0 * st.filled_t / cap.max(1.0), t(st.in_yesterday_t)),
        ServiceKind::Incinerator => format!("Burns {} a day at full staff · {:.1} MW now · {:.0} MWh yesterday · {} waiting", t(cap), st.mw, st.mwh_yesterday, t(st.backlog_t)),
        ServiceKind::RecyclingPlant => format!("Sorts {} a day at full staff · {} sorted, {} recovered yesterday · {} waiting", t(cap), t(st.done_yesterday_t), t(st.recovered_yesterday_t), t(st.backlog_t)),
        _ => format!("Ships {} a day at full staff · {} yesterday {} · {} waiting", t(cap), t(st.done_yesterday_t), if w.waste_ships_by_rail() { "by rail or ship" } else { "by road (no freight yard or port)" }, t(st.backlog_t)),
    };
    ui.label(egui::RichText::new(line).color(IVORY).small());
    let mut money = format!("Costs {:.0} yesterday by the tonne", st.cost_yesterday);
    if st.sales_yesterday > 0.0 {
        money += &format!(", sold {:.0} of materials", st.sales_yesterday);
    }
    if st.turned_away_yesterday > 0 {
        money += &format!(" · turned {} trucks away (full)", st.turned_away_yesterday);
    }
    hint(ui, &money);
    if info.staffing < 0.99 {
        ui.label(egui::RichText::new(format!("Staffed {:.0}%: it works at that share.", info.staffing * 100.0)).color(if info.staffing < 0.5 { WARN } else { GOLD }).small());
    }
    if kind == ServiceKind::Landfill && st.filled_t >= cap * 0.9 {
        ui.label(egui::RichText::new("Nearly full: build another landfill (or burn, sort or ship more) before the trucks are turned away.").color(WARN).small());
    }
}

/// City Data, Services, Trash.
pub fn page(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let tr = &w.society.trash;
    let d = &tr.disposal;
    let s = &tr.stats;
    let over = tr.bins.values().filter(|b| b.overflow() > 0.0).count();
    cards(ui, |ui| {
        kpi(ui, "made yesterday", egui::RichText::new(t(s.produced_yesterday / 1000.0)));
        kpi(ui, "collected by the city", egui::RichText::new(t(s.collected_city_yesterday / 1000.0)));
        kpi(ui, "by private haulers", egui::RichText::new(t(s.collected_private_yesterday / 1000.0)));
        kpi(ui, "overflowing buildings", super::tools::warn_if(over > 0, format!("{over}")));
        kpi(ui, "disposal yesterday", egui::RichText::new(format!("{:.0}", d.cost_yesterday.iter().sum::<f32>())));
        kpi(ui, "recycling sales", egui::RichText::new(format!("{:.0}", d.sales_yesterday)));
        kpi(ui, "in the depots' yards", super::tools::warn_if(d.blocked_yesterday > 0, t(d.held_t())));
        kpi(ui, "power from trash", egui::RichText::new(format!("{:.1} MW", w.waste_mw())));
    });
    if d.starter_on(w.clock.day()) {
        ui.label(egui::RichText::new(format!("Starter contract: the contractor lorries the depots' yards out of town at half price ({:.0} a tonne) and with no daily limit until day {}. After that, {:.0} a tonne and a daily limit per depot.", w.road_price(), d.starter_until + 1, nd_sim::waste::COST_ROAD)).color(GOLD).small());
    }
    if d.blocked_yesterday > 0 {
        ui.label(egui::RichText::new(format!("{} truckloads could not be unloaded yesterday: a depot's yard was full. The trucks stop collecting and the bins overflow. Build a landfill, incinerator, recycling plant or transfer station within 3 km of the depots.", d.blocked_yesterday)).color(WARN).small());
    }
    heading(ui, "Where it went yesterday");
    egui::Grid::new("trash_dest").striped(true).num_columns(4).show(ui, |ui| {
        for h in ["way", "tonnes", "cost", "a tonne"] {
            ui.label(egui::RichText::new(h).small().strong().color(GOLD));
        }
        ui.end_row();
        for dest in Dest::ALL {
            let (tt, c) = (d.t_yesterday[dest.index()], d.cost_yesterday[dest.index()]);
            ui.label(dest.label());
            ui.label(t(tt));
            ui.label(format!("{c:.0}"));
            ui.label(if tt > 0.05 { format!("{:.0}", c / tt) } else { "-".into() });
            ui.end_row();
        }
    });
    hint(ui, &format!("Ever: {} landfilled, {} burnt, {} recycled, {} by rail or ship, {} by lorry; {:.0} spent, {:.0} earned from materials.", t(d.t_total[0]), t(d.t_total[1]), t(d.t_total[2]), t(d.t_total[3]), t(d.t_total[4]), d.cost_total, d.sales_total));
    let sites = w.waste_sites();
    heading(ui, "Disposal sites");
    if sites.is_empty() {
        hint(ui, "None yet: the depots unload in their yards and a contractor hauls it all out of town by lorry, the dearest way. Buildings > Services > Sanitation: a landfill is cheapest per tonne but fills, stinks and poisons the ground; an incinerator makes power but smokes; a recycling plant employs the most and sells what it recovers; a transfer station ships it out by rail or ship.");
    } else {
        let mut fly = None;
        egui::Grid::new("trash_sites").striped(true).num_columns(5).show(ui, |ui| {
            for h in ["site", "doing", "staff", "cost yesterday", ""] {
                ui.label(egui::RichText::new(h).small().strong().color(GOLD));
            }
            ui.end_row();
            for s in &sites {
                ui.label(w.buildings.get(s.id).map_or("?".to_string(), |b| b.name.clone()));
                ui.label(egui::RichText::new(crate::render::waste_site_label(w, s)).small());
                ui.label(super::tools::warn_if(s.staffing < 0.5, format!("{:.0}%", s.staffing * 100.0)));
                ui.label(format!("{:.0}", d.sites.get(&s.id).map_or(0.0, |x| x.cost_yesterday)));
                if ui.small_button("go").clicked() {
                    fly = Some(s.pos);
                }
                ui.end_row();
            }
        });
        if let Some(at) = fly {
            p.fly_to = Some(bevy::math::Vec2::new(at.x, at.y));
        }
    }
    let depots = w.trash_depots();
    if !depots.is_empty() {
        heading(ui, "Depots");
        for (id, _, _, trucks) in depots {
            let name = w.buildings.get(id).map_or("?".to_string(), |b| b.name.clone());
            let held = d.yards.get(&id).copied().unwrap_or(0.0);
            hint(ui, &format!("{name}: {trucks} trucks crewed, {} in the yard", t(held)));
        }
    }
    if d.scavengers_yesterday > 0 || d.landfill_ill_total > 0 {
        hint(ui, &format!("Last night {} people sleeping rough picked through the landfills ({:.0} earned so far); rats from the tips have made {} people ill.", d.scavengers_yesterday, d.scavenged_total, d.landfill_ill_total));
    }
}
