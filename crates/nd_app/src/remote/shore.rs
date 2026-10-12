//! [waterfront] The shore by remote: waterfront lots, public beaches and
//! private jetties (`nd_sim::shore`).
//!
//! - `shore {}`: the summary (waterfront lots, vacant ones, beaches, jetties,
//!   the waterfront trades' pressures, whether parcels reach the water).
//! - `beaches {}`: every public beach with its state.
//! - `jetties {}`: every private jetty.
//! - `waterfront_parcels {on}`: switch parcels down to the water on or off
//!   for this city (a save from before has it off; lots follow at the next
//!   implement, and buildings whose parcels change shape are pulled down).
//!
//! The same JSON goes into `inspect` for lots (`shore`) and buildings
//! (`beach`, `jetty`).

use super::view::{key_str, opt_bool, round, v2};
use nd_sim::shore::{BeachState, Jetty};
use nd_sim::{BuildingId, LotId, World};
use serde_json::{json, Value};

/// A lot's edge on the water, or null inland.
pub fn lot_shore(w: &World, id: LotId) -> Value {
    let Some(l) = w.lots.get(id) else { return Value::Null };
    if !l.shore.is_waterfront() && !l.shore.beach {
        return Value::Null;
    }
    json!({
        "waterfront": l.shore.is_waterfront(), "frontage_m": round(l.shore.frontage), "seaward": v2(l.shore.seaward), "beach": l.shore.beach,
        "beach_state": w.beach_on(id).map(|(b, s)| beach_json(w, b, s)),
    })
}

pub fn beach_json(w: &World, b: BuildingId, s: &BeachState) -> Value {
    let lot = w.buildings.get(b).and_then(|x| w.lots.get(x.lot));
    json!({
        "building": key_str(b), "name": s.name, "pos": lot.map(|l| v2(l.center)), "area_m2": if s.area > 0.0 { Some(round(s.area)) } else { lot.map(|l| round(l.area())) },
        // [beaches] (H2) the lots of a merged beach
        "lots": s.lots.len().max(1), "members": s.members.iter().map(|m| key_str(*m)).collect::<Vec<_>>(),
        "access": s.access, "open": s.open, "usable": s.usable(), "note": s.note,
        "visitors": s.visitors, "swimmers": s.swimmers, "people_here": s.people_here, "draw": round(s.draw),
        "capacity": s.capacity, "crowded": s.crowded(), "catchment": s.catchment,
        "appeal": round(s.appeal), "contamination": round(s.contamination),
        "peak_today": s.peak_today, "visits_today": s.visits_today, "visits_yesterday": s.visits_yesterday,
        "towel_spots": lot.and_then(|l| l.park.as_ref()).map(|p| p.benches.len()),
    })
}

pub fn jetty_json(w: &World, b: BuildingId, j: &Jetty) -> Value {
    let owner = w.buildings.get(b);
    json!({
        "building": key_str(b), "name": owner.map(|x| x.name.clone()), "business": owner.and_then(|x| x.business.as_ref()).map(|z| z.kind.label()),
        "kind": j.kind.label(), "root": v2(j.root), "tip": v2(j.tip), "berth": v2(j.berth), "water": j.water.label(),
        "berths": j.kind.berths(), "built_day": j.built / 1440 + 1, "loads_landed": j.loads,
        "loads_coming": w.shore.inbound.iter().filter(|(x, _, _)| *x == b).count(),
    })
}

/// The beach and jetty of a building, for `inspect`.
pub fn building_extra(w: &World, b: BuildingId, v: &mut Value) {
    if let Some(s) = w.shore.beach(b) {
        v["beach"] = beach_json(w, b, s);
    }
    if let Some(j) = w.shore.jetties.get(&b) {
        v["jetty"] = jetty_json(w, b, j);
    }
    if let Some(l) = w.buildings.get(b).map(|x| x.lot) {
        let s = lot_shore(w, l);
        if !s.is_null() {
            v["shore"] = s;
        }
    }
}

pub fn command(name: &str, cmd: &Value, w: &mut World) -> Result<Value, String> {
    match name {
        "beaches" => Ok(json!({ "beaches": w.shore.beaches.iter().map(|(b, s)| beach_json(w, *b, s)).collect::<Vec<_>>(), "bathers_now": w.shore.bathers(), "appeal_now": round(w.beach_appeal_now()), "appeal_season": round(w.beach_appeal_season()) })),
        "jetties" => Ok(json!({ "jetties": w.shore.jetties.iter().map(|(b, j)| jetty_json(w, *b, j)).collect::<Vec<_>>() })),
        "waterfront_parcels" => {
            if let Some(on) = opt_bool(cmd, "on")? {
                w.set_waterfront_parcels(on);
            }
            Ok(json!({ "on": w.shore.reach_water, "parcels_on_water": w.parcels.parcels.iter().filter(|p| p.water_frontage >= nd_sim::zoning::LotShore::MIN_FRONTAGE).count(), "note": "lots follow at the next implement; a building whose parcel changes shape is pulled down" }))
        }
        _ => {
            let lots: Vec<(LotId, &nd_sim::Lot)> = w.lots.iter().filter(|(_, l)| l.shore.is_waterfront()).collect();
            let trades = ["beach_resort", "pier_restaurant", "beach_bar", "boat_rental"];
            let pressures: Vec<Value> = w
                .pressure
                .readings
                .iter()
                .filter(|r| trades.contains(&r.kind.key()))
                .map(|r| json!({ "kind": r.kind.key(), "wanted": round(r.wanted), "have": r.have, "gap": round(r.gap), "why": r.why, "held": r.held }))
                .collect();
            let open = |k: nd_sim::BusinessKind| w.buildings.values().filter(|b| b.business.as_ref().is_some_and(|z| z.kind == k && !z.closed)).count();
            Ok(json!({
                "parcels_reach_water": w.shore.reach_water,
                "waterfront_lots": lots.len(), "waterfront_vacant": lots.iter().filter(|(_, l)| l.building.is_none() && l.usable() && !l.shore.beach).count(),
                "waterfront_frontage_m": round(lots.iter().map(|(_, l)| l.shore.frontage).sum::<f32>()),
                "beaches": w.shore.beaches.len(), "beaches_open": w.shore.beaches.values().filter(|b| b.usable()).count(), "bathers_now": w.shore.bathers(),
                "jetties": w.shore.jetties.len(),
                "trades": {
                    "beach_resorts": open(nd_sim::BusinessKind::BeachResort), "pier_restaurants": open(nd_sim::BusinessKind::PierRestaurant),
                    "beach_bars": open(nd_sim::BusinessKind::BeachBar), "boat_rentals": open(nd_sim::BusinessKind::BoatRental),
                    "waterfront_hotels": w.buildings.values().filter(|b| b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::Hotel && !z.closed) && w.lots.get(b.lot).is_some_and(|l| l.shore.is_waterfront())).count(),
                },
                "pressures": pressures,
            }))
        }
    }
}
