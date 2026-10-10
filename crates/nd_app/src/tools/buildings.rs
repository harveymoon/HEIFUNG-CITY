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
    let Frame { click, ground, .. } = *f;
    // [free-placement] What a click here would do, for the panel and
    // the outline: any lot, or unzoned ground with road access.
    let what = tools.build_item();
    // [plot-hover] The remote API's pin stands in for the pointer.
    tools.build_here = match (tools.cursor_pin.or(ground), what) {
        (_, nd_sim::Placeable::Toll(_) | nd_sim::Placeable::Rail(_) | nd_sim::Placeable::Dock(_)) | (None, _) => None,
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
                        Some(b) => {
                            let mut s = format!("{} built: {:.1} MW for {:.0}; {:.0}/day to run.", what.label(), sim.world.plant_capacity_mw(b), paid, sim.world.plant_upkeep(b));
                            if let Some(g) = sim.world.plant_group_of(id) {
                                let mw: f32 = g.members.iter().filter_map(|m| sim.world.buildings.get(*m)).map(|m| sim.world.plant_capacity_mw(m)).sum();
                                s += &format!(" Part of {}: {} lots, {:.0} MW.", g.name, g.members.len(), mw);
                            }
                            s
                        }
                        // [dock-access] A dock or station off the roads says so.
                        None if sim.world.road_access(id) == Some(false) => format!("{} built for {:.0}. Not connected to the road network yet: draw a road to the marked end of its street stub.", what.label(), paid),
                        None => format!("{} built for {:.0}; {:.0}/day to run.", what.label(), paid, what.upkeep()),
                    }
                }
                Err(e) => format!("Cannot build: {e}."),
            };
        }
    }
}

/// The Buildings tool's preview: the lot under the cursor green or red,
/// the coverage radius round it for services (road distance; the ring is
/// the straight-line bound), and for rail pieces the footprint as placed.
pub(super) fn draw_build_preview(gizmos: &mut Gizmos, tools: &ToolState, cursor: &Cursor, sim: &Sim) {
    let Some(c) = cursor.ground else { return };
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
