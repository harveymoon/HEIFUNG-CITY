//! [lines] What is wrong with a bus line, for the Lines window: shown on the
//! line's row (a red or amber mark) and spelled out while it is edited.
//!
//! An adapter: the sim's own validation (`World::line_problems` and
//! `transit::LineProblem`, coming with fix/pt-transit) is not on main yet,
//! so this works the same checks out here. When it lands, [`bus_line`]
//! becomes a map over `w.line_problems(rid)` (`text: p.to_string()`,
//! `leg: p.leg()`, `blocks: p.blocks()`) and the rest goes.

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

impl Problem {
    fn blocking(text: impl Into<String>, leg: Option<usize>) -> Self {
        Self { text: text.into(), leg, blocks: true }
    }

    fn warning(text: impl Into<String>) -> Self {
        Self { text: text.into(), leg: None, blocks: false }
    }
}

/// A leg this many times longer than its stops are apart, plus the slack,
/// goes the long way round (a one-way street, a stop on the far kerb).
const LONG_WAY_FACTOR: f32 = 2.5;
const LONG_WAY_SLACK: f32 = 250.0;

/// Everything keeping line `rid` from running as drawn, or worth a warning.
pub fn bus_line(w: &World, rid: RouteId) -> Vec<Problem> {
    let Some(r) = w.transit.routes.get(rid) else { return Vec::new() };
    let n = r.stops.len();
    let mut out = Vec::new();
    if n < 2 {
        out.push(Problem::blocking("needs at least two stops", None));
    } else {
        for i in 0..n {
            if r.broken.contains(&i) {
                out.push(Problem::blocking(format!("no road from stop {} to {}: move one of them, or join the streets", i + 1, (i + 1) % n + 1), Some(i)));
                continue;
            }
            let direct = r.stops[i].pos.distance(r.stops[(i + 1) % n].pos);
            let metres = r.leg_length(i);
            if r.legs.len() == n && metres > direct * LONG_WAY_FACTOR + LONG_WAY_SLACK {
                out.push(Problem { text: format!("stop {} to {} is a {:.0} m drive for {:.0} m apart (a one-way street, or a stop on the far kerb?)", i + 1, (i + 1) % n + 1, metres, direct), leg: Some(i), blocks: false });
            }
        }
    }
    if r.buses == 0 {
        out.push(Problem::blocking("set to run no buses: + adds one", None));
    }
    let depots: Vec<nd_sim::BuildingId> = w.buildings.iter().filter(|(_, b)| b.working_depot()).map(|(id, _)| id).collect();
    if depots.is_empty() {
        out.push(Problem::blocking("no bus depot: build one (Build, Transit, Buses, Bus depot); buses start and end the day there", None));
    } else if n >= 2 {
        let (wanted, capacity) = fleet_balance(w);
        if wanted > capacity {
            out.push(Problem::warning(format!("the lines want {wanted} buses but the depots hold {capacity}: build another depot or run fewer buses")));
        }
        let out_now = w.traffic.cars.values().filter(|c| c.bus.as_ref().is_some_and(|b| b.route == rid)).count();
        let reach = out_now > 0 || depots.iter().any(|&d| depot_reaches(w, d, r.stops[0]));
        if !reach {
            out.push(Problem::blocking("no depot can reach stop 1 by road", None));
        }
    }
    let minute = w.clock.minute_of_day() as u32;
    if !nd_sim::transit::Transit::in_service(minute) {
        let hm = |m: u32| format!("{:02}:{:02}", m / 60, m % 60);
        out.push(Problem::warning(format!("night: buses run {} to {}", hm(nd_sim::transit::SERVICE_START), hm(nd_sim::transit::SERVICE_END))));
    }
    out
}

/// Buses wanted across the lines that can run, and how many the working
/// depots hold (their own bays and the truck lots they overflow into).
fn fleet_balance(w: &World) -> (u32, u32) {
    let wanted = w.transit.routes.values().filter(|r| r.runnable()).map(|r| r.buses as u32).sum();
    let capacity = w.buildings.iter().filter(|(_, b)| b.working_depot()).map(|(id, _)| nd_sim::transit::DEPOT_CAPACITY + w.depot_overflow(id)).sum();
    (wanted, capacity)
}

/// Can a bus leave depot `d` and drive to `stop`?
fn depot_reaches(w: &World, d: nd_sim::BuildingId, stop: nd_sim::transit::BusStop) -> bool {
    let Some(lot) = w.buildings.get(d).and_then(|b| w.lots.get(b.lot)) else { return false };
    let Some(seg) = lot.car_segment else { return false };
    w.traffic.net.find_route(seg, lot.car_point, stop.segment, stop.pos).is_some()
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
    // Re-checked every game hour too (buses out, depots full).
    let key = (generation, w.transit.version, w.transit.routes.len(), depots, service, w.clock.tick / 60);
    if let Some(c) = ctx.data(|d| d.get_temp::<Cache>(id)).filter(|c| c.key == key) {
        return c.problems;
    }
    let problems: std::collections::HashMap<RouteId, Vec<Problem>> = w.transit.routes.keys().map(|r| (r, bus_line(w, r))).collect();
    ctx.data_mut(|d| d.insert_temp(id, Cache { key, problems: problems.clone() }));
    problems
}
