//! The Rail tool: stations, freight yards and docks put down where clicked;
//! the previews of the piece or dock as it would land and of what still
//! wants joining.

use bevy::prelude::*;
use nd_sim::Vec2 as SVec2;

use super::input::Frame;
use super::{Preview, ToolState};
use crate::overlay::shapes::flat_at;
use crate::selection::Selection;
use crate::sim::Sim;

/// A dock, else a station or freight yard, at the click.
pub(super) fn input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection) {
    let Frame { click, ground, .. } = *f;
    if let (true, Some(p), Some(dock)) = (click, ground, tools.dock) {
        tools.status = match sim.world.place_port(dock, p) {
            Ok(id) => {
                *selection = Selection::Building(id);
                match sim.world.ports.port_of(id) {
                    Some(port) if port.works() => format!("{} built with its pier. Ships call from the next sailing.", dock.label()),
                    Some(port) if !port.road_connected => format!("{} built with its pier and an access road. Not connected to the road network yet: draw a road to the marked end.", dock.label()),
                    _ => format!("{} built, but no ship can reach it from off the map.", dock.label()),
                }
            }
            Err(e) => format!("Cannot build: {e}."),
        };
    } else if click {
        if let Some(p) = ground {
            let kind = tools.rail_kind;
            tools.status = match sim.world.place_rail_facility(kind, p) {
                Ok(id) => {
                    *selection = Selection::Building(id);
                    // `place_rail_facility` sets the link; should a sim change
                    // ever leave it off, say so rather than panic mid-click.
                    let link = sim.world.buildings.get(id).and_then(|b| b.rail.as_ref()).map(|l| (l.rail_connected, l.road_connected));
                    match link {
                        Some((true, true)) => format!("{} placed and connected. Trains call from the next headway.", kind.label()),
                        Some((true, false)) => format!("{} placed on the track; trains call. Not connected to the road network yet: draw a road to the marked end.", kind.label()),
                        Some((false, true)) => format!("{} placed by the street. Lay track onto its rail stub and it opens.", kind.label()),
                        Some((false, false)) => format!("{} placed. Lay track to its rail stub and a road to its street stub.", kind.label()),
                        None => format!("{} placed, but it reports no rail link; select it to check.", kind.label()),
                    }
                }
                Err(e) => format!("Cannot build: {e}."),
            };
        }
    }
}

/// The dock as it would land: footprint, pier, and its street side.
pub(super) fn dock_preview(gizmos: &mut Gizmos, p: &Preview) {
    let Preview { tools, cursor, sim, .. } = *p;
    let lift = |q: SVec2| super::lift(sim, q);
    let (cyan, red, white) = (super::CYAN, super::RED, super::WHITE);
    // The dock as it would land: footprint, pier, and its street side.
    if let (Some(c), Some(kind)) = (cursor.ground, tools.dock) {
        match sim.world.plan_port(kind, c) {
            Ok(port) => {
                let ok = Color::linear_rgb(0.3, 2.5, 0.6);
                let mut pts: Vec<Vec3> = port.footprint().iter().map(|&p| lift(p)).collect();
                if let Some(&first) = pts.first() { pts.push(first); }
                gizmos.linestrip(pts, ok);
                let [a, b] = port.pier;
                gizmos.line(lift(a), lift(b), white);
                gizmos.line(lift(b), lift(port.berth), cyan);
                // [dock-access] The access road it would lay inland.
                let [sa, sb] = sim.world.dock_stub(&port);
                gizmos.line(lift(sa), lift(sb), cyan);
                gizmos.circle(flat_at(lift(sb)), 6.0, cyan);
            }
            Err(_) => {
                gizmos.circle(flat_at(lift(c)), 12.0, red);
            }
        }
    }
    for port in sim.world.ports.ports.values().filter(|p| !p.works()) {
        let [a, b] = port.road_line();
        gizmos.line(lift(a) + Vec3::Y, lift(b) + Vec3::Y, red);
    }
}

/// The piece as it would land, connectors still to join, the railway.
pub(super) fn preview(gizmos: &mut Gizmos, p: &Preview) {
    let Preview { tools, cursor, sim, .. } = *p;
    let lift = |q: SVec2| super::lift(sim, q);
    let (cyan, red, white) = (super::CYAN, super::RED, super::WHITE);
    if let Some(c) = cursor.ground {
        // The piece as it would land: footprint, track side, street side.
        let (f, rail_fused, road_fused) = nd_sim::Facility::plan(tools.rail_kind, c, &sim.world.roads);
        let fp = f.footprint();
        // [fix/review-app] Red by the game's own rule, as the
        // Buildings tool's preview and placing it decide.
        let blocked = sim.world.piece_verdict(nd_sim::Placeable::Rail(tools.rail_kind), c).is_some_and(|v| v.is_err());
        let col = if blocked { red } else { Color::linear_rgb(0.3, 2.5, 0.6) };
        let mut pts: Vec<Vec3> = fp.iter().map(|&p| lift(p)).collect();
        if let Some(&first) = pts.first() { pts.push(first); }
        gizmos.linestrip(pts, col);
        let [ra, rb] = f.rail_line();
        gizmos.line(lift(ra), lift(rb), if rail_fused { white } else { Color::linear_rgb(1.6, 1.2, 0.3) });
        let [sa, sb] = f.road_line();
        gizmos.line(lift(sa), lift(sb), if road_fused { white } else { cyan });
    }
    // Connectors of placed pieces that still want joining.
    for (id, b) in sim.world.buildings.iter() {
        let Some(link) = &b.rail else { continue };
        if link.rail_connected && (link.road_connected || link.works()) {
            continue;
        }
        let Some(fac) = sim.world.facilities.values().find(|x| x.building == Some(id)) else { continue };
        if !link.rail_connected {
            let [a, bb] = fac.rail_line();
            gizmos.line(lift(a) + Vec3::Y, lift(bb) + Vec3::Y, red);
        }
        if !(link.road_connected || (link.kind.is_passenger() && link.walk_connected)) {
            let [a, bb] = fac.road_line();
            gizmos.line(lift(a) + Vec3::Y, lift(bb) + Vec3::Y, red);
        }
    }
    // The railway itself, so branches are easy to see while placing.
    for (sid, seg) in sim.world.roads.alive_segments() {
        if seg.is_rail() {
            let (a, b) = sim.world.roads.segment_points(sid);
            gizmos.line(lift(a), lift(b), Color::linear_rgb(1.6, 1.2, 0.3));
        }
    }
}
