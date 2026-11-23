//! [r14-foodtrucks] The inspector on a food truck: its type and menu, who
//! runs it, where it is and what it is doing, today's spots by the hour,
//! the takings and the stock.

use super::*;
use nd_sim::food_trucks::{TruckJob, TruckPlace};

fn hhmm(tick: u64) -> String {
    let m = tick % nd_sim::clock::MINUTES_PER_DAY;
    format!("{:02}:{:02}", m / 60, m % 60)
}

fn hours(from: u8, to: u8) -> String {
    format!("{:02}:00–{:02}:00", from % 24, to % 24)
}

pub(super) fn card(ui: &mut egui::Ui, w: &nd_sim::World, selection: &mut Selection, id: nd_sim::food_trucks::TruckId) {
    let Some(t) = w.food_trucks.trucks.get(id) else {
        *selection = Selection::None;
        return;
    };
    let row = t.kind.row();
    neon(ui, &t.name);
    ui.label(egui::RichText::new(format!("{}  ·  {}", capitalise(row.label), row.menu)).weak());
    let doing = match (t.place, t.job) {
        (TruckPlace::Pitch { open: true, .. }, _) => {
            let at = t.visits.last().map(|v| v.name.clone()).unwrap_or_default();
            format!("Open for business at {at}: {} in the queue.", w.food_trucks.queue_len(id))
        }
        (TruckPlace::Pitch { .. }, _) => "Parked at its spot, not open yet.".to_string(),
        (TruckPlace::Road(_), TruckJob::ToPitch { stint, .. }) => format!("Driving out to {}.", t.plan.get(stint as usize).map_or("its spot".to_string(), |s| s.name.clone())),
        (TruckPlace::Road(_), TruckJob::Leaving) => "Sold: leaving town.".to_string(),
        (TruckPlace::Road(_), _) => "Driving home.".to_string(),
        (TruckPlace::Base, _) => match t.current_stint() {
            Some((_, s)) => format!("At home. Next: {} at {}.", s.name, hours(s.from, s.to)),
            None => "At home for the day.".to_string(),
        },
    };
    ui.label(doing);
    ui.horizontal(|ui| {
        figure(ui, "meals today", t.meals_today.to_string(), IVORY);
        figure(ui, "takings", format!("${:.0}", t.takings_today), GOOD);
        figure(ui, "costs", format!("${:.0}", t.costs_today), IVORY);
        figure(ui, "stock", format!("{:.0}", t.stock), if t.stock < 10.0 { WARN } else { IVORY });
    });
    ui.label(egui::RichText::new(format!("${:.0} a serving (a little more to the well-off). Yesterday: {} meals, ${:.0} taken, ${:.0} spent.", row.price, t.meals_yesterday, t.takings_yesterday, t.costs_yesterday)).weak());
    if t.bought_local_today + t.bought_wholesale_today > 0.0 {
        ui.label(egui::RichText::new(format!("Today's ingredients: {:.0} servings' worth from shops and farms in town, {:.0} wholesale from out of town.", t.bought_local_today, t.bought_wholesale_today)).weak());
    }
    // Who runs it.
    if let Some(hh) = w.households.get(t.household) {
        section(ui, &format!("RUN BY THE HOUSEHOLD  ·  ${:.0} in the purse", hh.money));
        let people: Vec<nd_sim::CitizenId> = hh.members.iter().copied().filter(|c| w.citizens.contains_key(*c)).collect();
        crate::ui::portrait::grid(ui, w, &people, 6, selection);
        if let Some(c) = t.crew.and_then(|c| w.citizens.get(c)) {
            ui.label(egui::RichText::new(format!("{} is driving and serving today.", c.name)).weak());
        }
    }
    // Today's plan.
    if !t.plan.is_empty() {
        section(ui, "TODAY'S SPOTS");
        for s in &t.plan {
            ui.label(format!("{}  {} ({}): {}", hours(s.from, s.to), s.name, s.kind.label(), s.state.label()));
        }
    }
    if !t.visits.is_empty() {
        section(ui, "TRADED TODAY");
        for v in &t.visits {
            let until = v.closed.map_or("now".to_string(), hhmm);
            ui.label(format!("{}–{}  {}: {} meals, ${:.0}", hhmm(v.opened), until, v.name, v.meals, v.takings));
        }
    }
    if t.losing_days > 0 {
        ui.label(egui::RichText::new(format!("Losing money {} day{} running (sold after {}).", t.losing_days, if t.losing_days == 1 { "" } else { "s" }, nd_sim::food_trucks::LOSING_DAYS)).color(WARN));
    }
    hint(ui, "Food trucks go where the crowds are: parks and beaches on warm days, offices at lunch, the stadium on game nights, the bars late. A meal from one feeds the hungry like any other.");
}
