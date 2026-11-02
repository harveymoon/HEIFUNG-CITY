//! The Buildings tool: what a click here would put down (worked out once
//! a frame for the panel, the outline and `render::plot_hover`), placing
//! it, and the preview of docks, toll booths, rail pieces and coverage.

use bevy::prelude::*;
use nd_sim::Vec2 as SVec2;

use super::input::Frame;
use super::ToolState;
use crate::overlay::shapes::flat_at;
use crate::selection::Selection;
use crate::sim::Sim;
use crate::camera::Cursor;

/// The site under the pointer (or the remote pin) and a click.
pub(super) fn input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection) {
    // [dams] (H1) A hydro plant's second step: its wall.
    if let Some(step) = tools.dam {
        tools.build_here = None;
        dam_input(f, sim, tools, selection, step);
        return;
    }
    tools.dam_note = None;
    let Frame { click, ground, .. } = *f;
    // [free-placement] What a click here would do, for the panel and
    // the outline: any lot, or unzoned ground with road access.
    let what = tools.build_item();
    // [plot-hover] The remote API's pin stands in for the pointer.
    tools.build_here = match (tools.cursor_pin.or(ground), what) {
        (_, nd_sim::Placeable::Toll(_) | nd_sim::Placeable::Rail(_) | nd_sim::Placeable::Dock(_)) | (None, _) => None,
        // [airport] Fitted to the ground under the pointer.
        (Some(p), nd_sim::Placeable::Airport(size)) => Some(match airport_plan(&sim.world, size, p) {
            Ok(a) => {
                let heading = a.along.y.atan2(a.along.x).to_degrees().rem_euclid(180.0);
                (true, format!("runways at {heading:.0}°, the ground levelled to {:.0} m; {:.0} to build ({:.0} of it earthworks), {:.0}/day", a.level, size.cost() + a.earthwork_cost, a.earthwork_cost, size.upkeep_per_day()), a.footprint())
            }
            Err(why) => (false, why, Vec::new()),
        }),
        (Some(p), _) => {
            let w = &sim.world;
            // Joining unzoned parcels into a site is the dear part:
            // kept while the pointer stays on the same parcel.
            let key = w.parcels.at(p).map(|i| (what, i, w.parcels.version, w.lots.len()));
            let cached = match (&tools.build_site, key) {
                (Some((a, b, c, d, site)), Some(k)) if (*a, *b, *c, *d) == k => Some(site.clone()),
                _ => None,
            };
            let site = cached.unwrap_or_else(|| w.placement_site(what, p));
            if let Some((a, b, c, d)) = key {
                tools.build_site = Some((a, b, c, d, site.clone()));
            }
            // [plant-scale] With what it would be there: a plant's
            // MW, cost and upkeep, the park it would join.
            Some(match site {
                Ok(site) => {
                    // Worked out again only when the site, the
                    // buildings or the treasury move.
                    let vkey = {
                        use std::hash::{Hash, Hasher};
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        (format!("{what:?}"), key.map(|k| (k.1, k.2, k.3)), site.lot, site.parcels.clone(), w.buildings.len(), w.economy.treasury.round() as i64, w.clock.tick / 60).hash(&mut h);
                        h.finish()
                    };
                    let (ok, text) = match &tools.build_verdict {
                        Some((k, ok, text)) if *k == vkey => (*ok, text.clone()),
                        _ => crate::render::site_verdict(w, what, &site),
                    };
                    tools.build_verdict = Some((vkey, ok, text.clone()));
                    (ok, text, site.outline)
                }
                Err(why) => (false, why, Vec::new()),
            })
        }
    };
    // [airport] Shift-click: clear the streets across the site for it.
    if f.shift_click {
        if let (Some(p), nd_sim::Placeable::Airport(size)) = (ground, tools.build_item()) {
            let before = sim.world.economy.treasury;
            tools.status = match sim.world.place_airport_with(size, p, None, true) {
                Ok(id) => {
                    *selection = Selection::Building(id);
                    format!("{} built for {:.0}, the streets across its site cleared. Draw a road to its forecourt road if it is not joined up.", size.label(), before - sim.world.economy.treasury)
                }
                Err(e) => format!("Cannot build: {e}."),
            };
            return;
        }
    }
    if click {
        if let Some(p) = ground {
            let what = tools.build_item();
            // [tolls] A toll booth goes across the road, not on a lot.
            if let nd_sim::Placeable::Toll(size) = what {
                tools.status = match sim.world.place_toll(p, size) {
                    Ok(_) => format!("{} built for {:.0}; {:.0}/day to run. Toll {:.1} a car (set it in City Data, Economy, Taxes).", what.label(), what.cost(), what.upkeep(), size.default_price()),
                    Err(e) => format!("Cannot build: {e}."),
                };
                return;
            }
            let before = sim.world.economy.treasury;
            tools.status = match sim.world.place_building(what, p) {
                Ok(id) => {
                    *selection = Selection::Building(id);
                    // [clarity] Solar and wind are sized to their lot;
                    // [plant-groups] the price is what was paid.
                    let paid = before - sim.world.economy.treasury;
                    match sim.world.buildings.get(id).filter(|b| b.plant.is_some()) {
                        // [dams] (H1) A hydro plant: now its wall.
                        Some(b) if b.plant == Some(nd_sim::PowerPlantKind::Hydro) => {
                            let s = format!("{} built for {:.0}. Now its dam: click the bank by the plant, then the far bank (Esc: later).", b.name, paid);
                            sim.world.await_dam_wall(id);
                            tools.dam = Some(super::DamStep { plant: id, a: None });
                            s
                        }
                        Some(b) => {
                            let mut s = format!("{} built: {:.1} MW for {:.0}; {:.0}/day to run.", what.label(), sim.world.plant_capacity_mw(b), paid, sim.world.plant_upkeep(b));
                            if let Some(g) = sim.world.plant_group_of(id) {
                                let mw: f32 = g.members.iter().filter_map(|m| sim.world.buildings.get(*m)).map(|m| sim.world.plant_capacity_mw(m)).sum();
                                s += &format!(" Part of {}: {} lots, {:.0} MW.", g.name, g.members.len(), mw);
                            }
                            s
                        }
                        // [dock-access] A dock or station off the roads says so.
                        // (D7) Joined by a footpath only: what works and what needs a road.
                        None if sim.world.road_access(id) == Some(false) => match sim.world.access_problem(id).or_else(|| sim.world.access_note(id)) {
                            Some(why) if sim.world.footpath_only(id) == Some(true) => format!("{} built for {:.0}. It is {why}.", what.label(), paid),
                            _ => format!("{} built for {:.0}. Not connected to the road network yet: draw a road to the marked end of its street stub.", what.label(), paid),
                        },
                        // [airport] Its forecourt road needs joining to the city.
                        None if sim.world.airports.by_building(id).is_some_and(|(_, a)| !a.road_connected) => format!("{} built for {:.0}. Not connected to the road network yet: draw a road to its forecourt road.", what.label(), paid),
                        None => format!("{} built for {:.0}; {:.0}/day to run.", what.label(), paid, what.upkeep()),
                    }
                }
                Err(e) => format!("Cannot build: {e}."),
            };
        }
    }
}

/// [dams] (H1) The dam wall step: the first click sets the bank by the
/// plant, the second the far bank and builds it; a right click takes the
/// first bank back. The wall under the pointer is read out live (length,
/// water spanned, cost, the power the plant would make).
fn dam_input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection, step: super::DamStep) {
    let w = &sim.world;
    let Some(name) = w.buildings.get(step.plant).map(|b| b.name.clone()) else {
        tools.dam = None;
        tools.dam_note = None;
        return;
    };
    if f.right_click && step.a.is_some() {
        tools.dam = Some(super::DamStep { a: None, ..step });
        return;
    }
    let at = tools.cursor_pin.or(f.ground);
    match (step.a, at) {
        (None, Some(p)) => {
            tools.dam_note = Some(match w.dam_end_problem(p) {
                Some(why) => (false, why),
                None => (true, format!("first bank here; then click the far bank (one end within {:.0} m of {name})", nd_sim::dams::DAM_NEAR_PLANT)),
            });
            if f.click && w.dam_end_problem(p).is_none() {
                tools.dam = Some(super::DamStep { a: Some(p), ..step });
            }
        }
        (Some(a), Some(p)) => {
            let plan = w.plan_dam_wall(step.plant, a, p);
            tools.dam_note = Some(match &plan {
                Ok(d) => (true, dam_readout(d)),
                Err(why) => (false, why.clone()),
            });
            if f.click {
                tools.status = dam_click(sim, tools, selection, step.plant, a, p);
            }
        }
        (_, None) => {}
    }
}

/// Build the wall from `a` to `b` for the step's plant: what to say.
pub(crate) fn dam_click(sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection, plant: nd_sim::BuildingId, a: SVec2, b: SVec2) -> String {
    let name = sim.world.buildings.get(plant).map_or_else(String::new, |b| b.name.clone());
    match sim.world.build_dam_wall(plant, a, b) {
        Ok(d) => {
            tools.dam = None;
            tools.dam_note = None;
            *selection = Selection::Building(plant);
            format!("Dam wall of {name} built: {:.0} m for {:.0}, a footpath along its crest. About {:.1} MW expected.", d.length, d.cost, d.expected_mw)
        }
        Err(e) => format!("Cannot build the wall: {e}."),
    }
}

/// The live read-out of a wall: its size, cost and power.
pub(crate) fn dam_readout(d: &nd_sim::dams::DamPlan) -> String {
    format!("wall {:.0} m ({:.0} m of it over the water) · costs {:.0} · about {:.1} MW expected", d.length, d.span, d.cost, d.expected_mw)
}

/// [dams] The wall being drawn: the arch from the first bank to the
/// pointer, green when it can go, red when not, and the plant's reach.
fn draw_dam_preview(gizmos: &mut Gizmos, tools: &ToolState, step: super::DamStep, c: SVec2, sim: &Sim) {
    let w = &sim.world;
    let lift = |p: SVec2| Vec3::new(p.x, 1.2 + w.surface_height(p).max(nd_sim::landscape::BANK_HEIGHT), p.y);
    let ok = tools.dam_note.as_ref().is_some_and(|n| n.0);
    let col = if ok { Color::linear_rgb(0.3, 2.5, 0.6) } else { Color::linear_rgb(3.0, 0.2, 0.4) };
    let cyan = Color::linear_rgb(0.3, 2.0, 3.0);
    // Where one end must be: round the plant's lot.
    if let Some(lot) = w.buildings.get(step.plant).and_then(|b| w.lots.get(b.lot)) {
        let r = lot.polygon.iter().map(|p| p.distance(lot.center)).fold(0.0, f32::max) + nd_sim::dams::DAM_NEAR_PLANT;
        gizmos.circle(flat_at(lift(lot.center)), r, cyan.with_alpha(0.35));
    }
    match step.a {
        None => {
            gizmos.circle(flat_at(lift(c)), 4.0, col);
        }
        Some(a) => {
            let upstream = w.plan_dam_wall(step.plant, a, c).map(|d| d.upstream).unwrap_or(SVec2::ZERO);
            let bow = (a.distance(c) * 0.5 * 0.32).clamp(3.0, 22.0);
            let y = nd_sim::landscape::WATER_LEVEL + nd_sim::landscape::BRIDGE_CLEARANCE + 0.5;
            let pts: Vec<Vec3> = nd_sim::dams::crest_between(a, c, upstream, bow, 16).into_iter().map(|p| Vec3::new(p.x, y, p.y)).collect();
            gizmos.linestrip(pts, col);
            gizmos.circle(flat_at(lift(a)), 4.0, col);
            gizmos.circle(flat_at(lift(c)), 4.0, col);
        }
    }
}

/// The Buildings tool's preview: the lot under the cursor green or red,
/// the coverage radius round it for services (road distance; the ring is
/// the straight-line bound), and for rail pieces the footprint as placed.
pub(super) fn draw_build_preview(gizmos: &mut Gizmos, tools: &ToolState, cursor: &Cursor, sim: &Sim) {
    let Some(c) = tools.cursor_pin.filter(|_| tools.dam.is_some()).or(cursor.ground) else { return };
    if let Some(step) = tools.dam {
        draw_dam_preview(gizmos, tools, step, c, sim);
        return;
    }
    let w = &sim.world;
    let what = tools.build_item();
    let lift = |p: SVec2| Vec3::new(p.x, 0.8, p.y);
    let green = Color::linear_rgb(0.3, 2.5, 0.6);
    let red = Color::linear_rgb(3.0, 0.2, 0.4);
    if let nd_sim::Placeable::Dock(kind) = what {
        match w.plan_port(kind, c) {
            Ok(port) => {
                let mut pts: Vec<Vec3> = port.footprint().iter().map(|&p| lift(p)).collect();
                if let Some(&first) = pts.first() { pts.push(first); }
                gizmos.linestrip(pts, green);
                let [a, b] = port.pier;
                gizmos.line(lift(a), lift(b), Color::linear_rgb(3.0, 3.0, 3.0));
                // [dock-access] The access road it would lay, and its free end.
                let probe = port.front_mid() - port.seaward * nd_sim::rail::ROAD_GAP;
                if !w.roads.nearest_street(probe).is_some_and(|(_, _, q)| q.distance(probe) <= nd_sim::rail::ROAD_EDGE_FUSE) {
                    let [sa, sb] = w.dock_stub(&port);
                    let cyan = Color::linear_rgb(0.3, 2.0, 3.0);
                    gizmos.line(lift(sa), lift(sb), cyan);
                    gizmos.circle(flat_at(lift(sb)), 6.0, cyan);
                }
            }
            Err(_) => {
                gizmos.circle(flat_at(lift(c)), 12.0, red);
            }
        }
        return;
    }
    // [tolls] The booth line across the road where it would stand.
    if let nd_sim::Placeable::Toll(size) = what {
        match nd_sim::tolls::site(&w.roads, c, size) {
            Ok((sid, at, dir)) => {
                let hw = w.roads.segment(sid).half_width() + if size == nd_sim::tolls::TollSize::Plaza { 8.0 } else { 0.5 };
                let right = SVec2::new(-dir.y, dir.x);
                let y = |p: SVec2| Vec3::new(p.x, 1.0 + w.surface_height(p), p.y);
                let near = w.tolls.booths.iter().any(|b| b.pos.distance(at) < nd_sim::tolls::SPACING_M);
                // [toll-fit] Priced by the road it would stand on.
                let col = if near || w.economy.treasury < size.cost_for(w.roads.segment(sid).lanes()) { red } else { green };
                gizmos.line(y(at - right * hw), y(at + right * hw), col);
                for u in [-4.0f32, 4.0] {
                    gizmos.line(y(at + dir * u - right * hw), y(at + dir * u + right * hw), col.with_alpha(0.5));
                }
                let flare = size.flare().max(10.0);
                gizmos.line(y(at - dir * flare), y(at + dir * flare), col.with_alpha(0.4));
            }
            Err(_) => {
                gizmos.circle(flat_at(lift(c)), 6.0, red);
            }
        }
        return;
    }
    // [airport] Its runways and the approach surfaces off their ends.
    if let nd_sim::Placeable::Airport(size) = what {
        match airport_plan(w, size, c) {
            Ok(a) => {
                let y = |p: SVec2| Vec3::new(p.x, w.surface_height(p) + 1.5, p.y);
                for r in size.runways() {
                    let [p, q] = a.runway_ends(r);
                    gizmos.line(y(p), y(q), Color::linear_rgb(3.0, 3.0, 3.0));
                }
                let cyan = Color::linear_rgb(0.3, 2.0, 3.0);
                for quad in nd_sim::airport::approach_quads(size, a.center, a.along, 1500.0) {
                    let mut pts: Vec<Vec3> = quad.iter().map(|&p| y(p)).collect();
                    pts.push(pts[0]);
                    gizmos.linestrip(pts, cyan.with_alpha(0.5));
                }
                let [sa, sb] = a.forecourt();
                gizmos.line(y(sa), y(sb), cyan);
            }
            Err(_) => {
                gizmos.circle(flat_at(lift(c)), 30.0, red);
            }
        }
        return;
    }
    if let nd_sim::Placeable::Rail(kind) = what {
        let (f, _, _) = nd_sim::Facility::plan(kind, c, &w.roads);
        let fp = f.footprint();
        let blocked = w.piece_verdict(what, c).is_some_and(|v| v.is_err());
        let mut pts: Vec<Vec3> = fp.iter().map(|&p| lift(p)).collect();
        if let Some(&first) = pts.first() { pts.push(first); }
        gizmos.linestrip(pts, if blocked { red } else { green });
        return;
    }
    // [free-placement] The lot under the pointer, or the unzoned parcels a
    // lot would be made of (worked out once a frame in `tool_input`).
    // [plot-hover] Its outline and light wall are drawn by
    // `render::plot_hover`; here only the coverage rings.
    let Some((_, _, outline)) = &tools.build_here else { return };
    if outline.len() < 3 {
        return;
    }
    let r = what.radius();
    if r > 0.0 {
        let centre = nd_sim::zoning::inner_center(outline);
        let col = Color::linear_rgb(0.4, 1.4, 2.4);
        gizmos.circle(flat_at(lift(centre)), r, col);
        gizmos.circle(flat_at(lift(centre)), (r - nd_sim::services::COVERAGE_FADE).max(1.0), col.with_alpha(0.5));
    }
}

/// [airport] `World::plan_airport` for the pointer, worked out again only
/// when it moves 10 m or the city under it changes (it tries every heading).
fn airport_plan(w: &nd_sim::World, size: nd_sim::airport::AirportSize, p: SVec2) -> Result<nd_sim::airport::Airport, String> {
    use std::hash::{Hash, Hasher};
    static CACHE: std::sync::Mutex<Option<(u64, Result<nd_sim::airport::Airport, String>)>> = std::sync::Mutex::new(None);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (size, (p.x / 10.0).round() as i32, (p.y / 10.0).round() as i32, w.buildings.len(), w.roads.version, w.lots.len(), w.airports.airports.len(), w.economy.treasury >= size.cost()).hash(&mut h);
    let key = h.finish();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, r)) = cache.as_ref() {
        if *k == key {
            return r.clone();
        }
    }
    let r = w.plan_airport(size, p, None);
    *cache = Some((key, r.clone()));
    r
}
