//! [lines] What is wrong with a bus line, for the Lines window: shown on the
//! line's row (a red or amber mark) and spelled out while it is edited.
//!
//! The sim's own validation (`World::line_problems`, `transit::LineProblem`)
//! in the window's terms, cached per edit.

use nd_sim::{RouteId, World};

/// One problem.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub text: String,
    /// The leg it is about (stop `leg` to the next), if one.
    pub leg: Option<usize>,
    /// It keeps the line from running at all (else a warning).
    pub blocks: bool,
}

/// Everything keeping line `rid` from running as drawn, or worth a
/// warning: the sim's own `World::line_problems`.
pub fn bus_line(w: &World, rid: RouteId) -> Vec<Problem> {
    w.line_problems(rid).into_iter().map(|p| Problem { text: p.to_string(), leg: p.leg(), blocks: p.blocks() }).collect()
}

/// The problems of every line, worked out once per edit of the lines (the
/// depot checks route a bus), cached in egui's memory.
pub fn cached(ctx: &bevy_egui::egui::Context, w: &World, generation: u64) -> std::collections::HashMap<RouteId, Vec<Problem>> {
    #[derive(Clone)]
    struct Cache {
        key: (u64, u64, usize, usize, bool, u64),
        problems: std::collections::HashMap<RouteId, Vec<Problem>>,
    }
    let id = bevy_egui::egui::Id::new("lines_problems");
    let depots = w.buildings.values().filter(|b| b.working_depot()).count();
    let service = nd_sim::transit::Transit::in_service(w.clock.minute_of_day() as u32);
    // Re-checked every game quarter hour too (buses out, depots full, a
    // timetable part changing: [r4-timetable]).
    let key = (generation, w.transit.version, w.transit.routes.len(), depots, service, w.clock.tick / 15);
    if let Some(c) = ctx.data(|d| d.get_temp::<Cache>(id)).filter(|c| c.key == key) {
        return c.problems;
    }
    let problems: std::collections::HashMap<RouteId, Vec<Problem>> = w.transit.routes.keys().map(|r| (r, bus_line(w, r))).collect();
    ctx.data_mut(|d| d.insert_temp(id, Cache { key, problems: problems.clone() }));
    problems
}
