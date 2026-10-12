//! `place` and `plant_preview`: buildings, stations, docks and tolls.

use nd_sim::zoning::point_in_polygon;
use nd_sim::{LotId, RailFacility, Vec2 as SVec2};
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use super::{land, taxes};
use crate::tools::Selection;

/// The lot under `p`, or failing that the nearest vacant one within reach:
/// a script aims at a block, not at a parcel's exact polygon.
pub(super) fn lot_near(w: &nd_sim::World, p: SVec2) -> Option<LotId> {
    if let Some((id, _)) = w.lots.iter().find(|(_, l)| point_in_polygon(p, &l.polygon)) {
        return Some(id);
    }
    w.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable()).map(|(id, l)| (id, l.center.distance(p))).filter(|(_, d)| *d < 80.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
}

pub(super) fn place(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let kind = req_str(cmd, "kind")?.trim().to_ascii_lowercase();
    let p = req_point(cmd)?;
    let w = &mut ctx.sim.world;
    let (id, note) = match kind.as_str() {
        "station" | "freight_yard" | "yard" | "central_station" | "central" | "main_station" => {
            let fac = match kind.as_str() {
                "station" => RailFacility::Station,
                "freight_yard" | "yard" => RailFacility::FreightYard,
                _ => RailFacility::Central,
            };
            let id = w.place_rail_facility(fac, p)?;
            let link = w.buildings.get(id).and_then(|b| b.rail.as_ref()).map(|l| (l.rail_connected, l.road_connected));
            let note = match link {
                Some((true, true)) => "placed and connected",
                Some((true, false)) => "placed on the track; trains call, nobody gets on or off until a road reaches the marked end of its street stub (road_access.stub_ends)",
                Some((false, true)) => "placed by the street; lay track onto its rail stub",
                Some((false, false)) => "placed; needs track to its rail stub and a road to its street stub",
                None => "placed",
            };
            (id, note.to_string())
        }
        "parking" => {
            let lot = lot_near(w, p).ok_or("no lot there")?;
            (w.build_parking(lot).ok_or("cannot build parking there: the lot is taken, landlocked or too small")?, "parking built".into())
        }
        // [tolls] Across the road at the point: not a building.
        other if nd_sim::tolls::TollSize::from_key(other).is_some() => {
            let size = nd_sim::tolls::TollSize::from_key(other).unwrap();
            let id = w.place_toll(p, size)?;
            let row = w.tolls.get(id).map(|b| taxes::booth_row(w, b)).unwrap_or(Value::Null);
            // [toll-fit] Priced by the lanes of its road.
            let lanes = w.tolls.get(id).map_or(2, |b| b.paid_lanes);
            let upkeep = w.tolls.get(id).map_or(size.upkeep_per_day(), |b| b.upkeep_per_day());
            let note = format!("{} across {lanes} lanes built for {:.0}; {:.0}/day; toll {:.1} (lorries double)", size.label(), size.cost_for(lanes), upkeep, size.default_price());
            ctx.tools.status = note.clone();
            return Ok(json!({ "toll": id, "message": note, "row": row, "treasury": round(w.economy.treasury) }));
        }
        other if land::dock_of(other).is_some() => {
            let dock = land::dock_of(other).unwrap();
            let id = w.place_port(dock, p)?;
            let port = w.ports.port_of(id).cloned();
            let note = match port {
                Some(p) if p.works() => format!("{} built with its pier; ships call from the next sailing", dock.label()),
                Some(p) if !p.road_connected => format!("{} built with its pier and an access road; not connected to the road network: draw a road to the marked end of its stub (road_access.stub_ends)", dock.label()),
                _ => format!("{} built, but no ship can reach it from off the map", dock.label()),
            };
            (id, note)
        }
        other => {
            let tier = opt_str(cmd, "tier")?.map(|t| nd_sim::services::parse_tier(t).ok_or_else(|| format!("unknown tier {t:?}; low, mid or high"))).transpose()?;
            let what = nd_sim::Placeable::parse(other, tier).ok_or_else(|| format!("unknown kind {other:?}; one of station, freight_yard, depot, parking, coal, solar, wind, nuclear, hydro, tidal, geothermal, battery_small, battery_medium, battery_large, police, fire, hospital (clinic), school, dispatch, monument, stadium, vertiport (services take tier low|mid|high or a :tier suffix)"))?;
            // [free-placement] Any lot or unzoned ground with road access at
            // the point (a lot is made for it there); off both, the nearest
            // vacant lot within 80 m. `check: true` only says what would happen.
            let site = w.placement_site(what, p);
            if opt_bool(cmd, "check")?.unwrap_or(false) {
                return Ok(match (site, w.placement_problem_at(what, p)) {
                    (Ok(site), Ok(_)) => json!({ "ok": true, "note": site.note(), "lot": site.lot.map(key_str), "parcels": site.parcels.len(), "leaves_zone": site.leaves_zone.map(|l| l.label()), "outline": points(&site.outline) }),
                    (_, Err(why)) => json!({ "ok": false, "reason": why }),
                    (Err(why), _) => json!({ "ok": false, "reason": why }),
                });
            }
            let before = w.economy.treasury;
            // [dams] `await_wall: true`: a hydro plant waits for its wall, as the tool places it.
            let await_wall = opt_bool(cmd, "await_wall")?.unwrap_or(false);
            let (id, here) = match site {
                Ok(site) => (w.place_building(what, p)?, site.note()),
                Err(_) => {
                    let lot = lot_near(w, p).ok_or("no lot or usable parcel there: aim at ground along a road")?;
                    (w.place_on_lot(what, lot)?, "nearest lot".into())
                }
            };
            if await_wall {
                w.await_dam_wall(id);
            }
            // [plant-groups] What was paid, and a plant's MW on its lot and its park.
            let paid = before - w.economy.treasury;
            let note = match w.buildings.get(id).filter(|b| b.plant.is_some()) {
                Some(b) => {
                    let mut s = format!("{} built: {:.1} MW for {:.0}; {:.0}/day ({here})", what.label(), w.plant_capacity_mw(b), paid, w.plant_upkeep(b));
                    if let Some(g) = w.plant_group_of(id) {
                        s += &format!("; part of {} ({} lots)", g.name, g.members.len());
                    }
                    s
                }
                None => format!("{} built for {:.0}; {:.0}/day ({here})", what.label(), paid, what.upkeep()),
            };
            (id, note)
        }
    };
    *ctx.selection = Selection::Building(id);
    ctx.tools.status = note.clone();
    let b = ctx.sim.world.buildings.get(id).map(|b| building_row(&ctx.sim.world, id, b)).unwrap_or(Value::Null);
    let mut out = json!({ "building": key_str(id), "message": note, "row": b, "treasury": round(ctx.sim.world.economy.treasury) });
    // [dock-access] A dock or station: connected, and the stub's free end.
    if let Some(connected) = ctx.sim.world.road_access(id) {
        out["road_access"] = access_json(&ctx.sim.world, id, connected);
    }
    Ok(out)
}

/// [plant-groups] What a plant `kind` would be on the lot at `x`,`y`
/// before placing it: its MW, cost and upkeep there, the Sunfield or Wind
/// Park it would join, and why it cannot go (if not). Read-only.
pub(super) fn plant_preview(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let kind = req_str(cmd, "kind")?.trim().to_ascii_lowercase();
    let p = req_point(cmd)?;
    let w = &ctx.sim.world;
    let Some(nd_sim::Placeable::Plant(k)) = nd_sim::Placeable::parse(&kind, None) else { return Err(format!("{kind:?} is not a power plant")) };
    // [plant-scale] The site a click there takes: a lot, or unzoned ground
    // (`placement_site`), so the figures are what the click charges.
    let site = w.placement_site(nd_sim::Placeable::Plant(k), p)?;
    let v = w.plant_preview_site(k, &site).ok_or("no road access to front")?;
    Ok(json!({
        "lot": site.lot.map(key_str), "unzoned_parcels": site.parcels.len(), "kind": kind, "area_m2": round(v.area), "size_scale": round(v.scale), "capacity_mw": round(v.capacity_mw), "site": round(v.site), "now_mw": round(v.now_mw),
        "cost": round(v.cost), "upkeep_per_day": round(v.upkeep_per_day), "min_area": k.min_area(), "max_area": k.max_area(), "small_lot_joins": v.small_lot_joins,
        "joins": v.join.as_ref().and_then(|j| j.joins.clone()), "joined_lots": v.join.as_ref().map(|j| j.members.len() + 1), "joined_area_m2": v.join.as_ref().map(|j| round(j.area)), "joined_capacity_mw": v.join.as_ref().map(|j| round(j.capacity_mw)), "adds_mw": v.join.as_ref().map(|j| round(j.capacity_mw - j.before_mw)),
        "outline": points(&site.outline),
        "ok": v.problem.is_none(), "problem": v.problem,
    }))
}
