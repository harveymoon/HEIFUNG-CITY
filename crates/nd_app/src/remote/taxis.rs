//! [taxis] The taxi and cargo hub sections of `report` and `inspect`.

use super::view::{key_str, round, v2};
use nd_sim::cargo_hub::HubState;
use nd_sim::taxi::{CabJob, CabPlace};
use nd_sim::{BuildingId, World};
use serde_json::{json, Value};

/// `report.transit.taxis`: companies, the fleet by what it is doing, the
/// day's rides and fares, riders waiting, and the demand nobody served.
pub fn taxi_report(w: &World) -> Value {
    let t = &w.taxis;
    let s = &t.stats;
    let k = t.counts();
    let companies: Vec<Value> = w.taxi_companies().into_iter().map(|c| company_json(w, c)).collect();
    json!({
        "companies": companies,
        "cabs": k.cabs, "on_duty": k.on_duty, "in_yard": k.in_yard, "at_ranks": k.at_ranks, "at_kerb": k.at_kerb, "on_road": k.on_road,
        "carrying": k.carrying, "to_pickup": k.to_pickup,
        "riders_waiting": t.requests.len(),
        "rides_today": s.rides_today, "rides_yesterday": s.rides_yesterday,
        "fares_today": round(s.fares_today), "fares_yesterday": round(s.fares_yesterday),
        "mean_wait_min_today": if s.rides_today > 0 { round(s.wait_minutes_today / s.rides_today as f32) } else { 0.0 },
        "mean_ride_m_today": if s.rides_today > 0 { round(s.fare_metres_today / s.rides_today as f32) } else { 0.0 },
        "drunk_rides_today": s.drunk_rides_today, "night_rides_today": s.night_rides_today,
        "gave_up_today": s.gave_up_today, "gave_up_yesterday": s.gave_up_yesterday,
        "unserved_demand_today": t.latent_today, "unserved_demand_yesterday": t.latent_yesterday,
        "companies_founded_total": s.companies_founded_total, "cabs_bought_total": s.cabs_bought_total, "cabs_sold_total": s.cabs_sold_total,
        "cabs_recovered_total": s.cabs_recovered_total,
        "rule": format!(
            "a company opens when {} trips in a day would have taken a cab and none was in reach ({} more per company in town); fare {:.1} + {:.1}/km; riders give up after {} min at the kerb",
            nd_sim::taxi::FOUND_DEMAND, nd_sim::taxi::FOUND_DEMAND, nd_sim::taxi::FLAG_FARE, nd_sim::taxi::FARE_PER_KM, nd_sim::taxi::WAIT_GIVE_UP
        ),
    })
}

/// One taxi company and its cabs.
pub fn company_json(w: &World, c: BuildingId) -> Value {
    let b = w.buildings.get(c);
    let cabs: Vec<Value> = w
        .taxis
        .fleet(c)
        .map(|(id, cab)| {
            let pos = match cab.place {
                CabPlace::Road(car) => w.traffic.cars.get(car).map(|x| x.pos),
                CabPlace::Yard => w.building_lot(c).map(|l| l.center),
                p => p.street_pose().map(|x| x.0),
            };
            let rank = match (cab.place, cab.job) {
                (CabPlace::Rank { rank, .. }, _) | (_, CabJob::ToRank { rank }) => Value::String(key_str(rank)),
                _ => Value::Null,
            };
            let car = match cab.place {
                CabPlace::Road(car) => Value::String(key_str(car)),
                _ => Value::Null,
            };
            json!({
                "id": key_str(id), "number": cab.number, "place": cab.place.label(), "job": cab.job.label(), "on_duty": cab.on_duty,
                "rider": cab.job.rider().map(key_str), "car": car, "rank": rank,
                "body": cab.model.body.label(), "pos": pos.map_or(Value::Null, v2),
            })
        })
        .collect();
    let (missed, missed_y) = w.taxis.missed.get(&c).copied().unwrap_or((0, 0));
    let (rides, rides_y) = w.taxis.rides_by_company.get(&c).copied().unwrap_or((0, 0));
    let bz = b.and_then(|b| b.business.as_ref());
    json!({
        "id": key_str(c), "name": b.map(|b| b.name.clone()), "pos": w.building_lot(c).map_or(Value::Null, |l| v2(l.center)),
        "money": bz.map(|z| round(z.money)), "revenue_today": bz.map(|z| round(z.revenue_today)),
        "cabs": cabs, "fleet": w.taxis.fleet_size(c), "rides_today": rides, "rides_yesterday": rides_y, "missed_today": missed, "missed_yesterday": missed_y,
    })
}

/// `report.transit.cargo_hubs`: each hub's loads by stage and the day's freight.
pub fn hub_report(w: &World) -> Value {
    let s = &w.cargo_hubs.stats;
    let hubs: Vec<Value> = w.cargo_hubs.hubs.iter().map(|(id, h)| hub_json(w, *id, h)).collect();
    json!({
        "hubs": hubs,
        "received_today": s.received_today, "received_units_today": round(s.received_units_today), "forwarded_today": s.forwarded_today,
        "semis_in_today": s.trunks_in_today, "semis_out_today": s.trunks_out_today,
        "long_haul_trips_saved_today": s.long_haul_saved_today, "long_haul_trips_saved_yesterday": s.long_haul_saved_yesterday, "long_haul_trips_saved_total": s.long_haul_saved_total,
        "handling_fees_today": round(s.fees_today), "forwarded_total": s.forwarded_total,
        "rule": format!(
            "businesses within {:.0} m of a hub import through it: by sea or rail when a port or yard works (one within {:.0} m unloads straight into the hub), else in shared semis of {:.0} units from the edge; the hub's own lorries deliver on",
            nd_sim::cargo_hub::HUB_REACH, nd_sim::cargo_hub::SIDING_REACH, nd_sim::cargo_hub::TRUNK_LOAD
        ),
    })
}

pub fn hub_json(w: &World, id: BuildingId, h: &HubState) -> Value {
    let [waiting, trunk, inbound, ready] = h.units();
    let (yard, port) = w.hub_links(id);
    json!({
        "id": key_str(id), "name": w.buildings.get(id).map(|b| b.name.clone()), "works": w.cargo_hub_works(id),
        "pos": w.building_lot(id).map_or(Value::Null, |l| v2(l.center)),
        "units": { "waiting_for_semi": round(waiting), "on_semi": round(trunk), "inbound_rail_or_sea": round(inbound), "in_shed": round(ready), "exports_gathered": round(h.outbound) },
        "loads": { "waiting_for_semi": h.waiting.len(), "on_semi": h.trunk.len(), "inbound_rail_or_sea": h.inbound.len(), "in_shed": h.ready.len() },
        "semi_on_the_way": h.trunk_car.map(key_str),
        "lorry_bays": w.fleet_capacity(id), "lorries_parked": w.fleet_parked(id),
        "siding_yard": yard.map(key_str), "quay_port": port.map(key_str),
    })
}

/// Extra fields for `inspect` of a taxi company or a cargo hub.
pub fn building_extra(w: &World, id: BuildingId) -> Value {
    if let Some(h) = w.cargo_hubs.hubs.get(&id) {
        return json!({ "cargo_hub": hub_json(w, id, h) });
    }
    if w.buildings.get(id).and_then(|b| b.business.as_ref()).is_some_and(|z| z.kind == nd_sim::BusinessKind::TaxiCompany) {
        return json!({ "taxi_company": company_json(w, id) });
    }
    Value::Null
}
