//! [fire-scene] Emergency units in the API: `inspect {unit}`, `select
//! {unit}`, the `units` list, and what a fire engine's crew is doing on
//! scene (`fire_scene`).

use nd_procgen::BuildingRules;
use nd_sim::services::{IncidentKind, UnitKind};
use nd_sim::{UnitId, World};
use serde_json::{json, Value};

use super::view::{building_name, key_str, round, v2};
use crate::fire_scene::{engine_scene, fire_progress, fire_word, minutes_to_out, unit_doing, unit_incident, unit_pose, unit_title};

fn p3(p: bevy::prelude::Vec3) -> Value {
    json!([round(p.x), round(p.y), round(p.z)])
}

/// One unit in full: who and where, what it is doing, its crew, the
/// incident, and at a fire the scene (crew places, ladder, hoses, water).
pub fn unit_detail(sim: &crate::sim::Sim, uid: UnitId, rules: &BuildingRules) -> Option<Value> {
    let (w, frac) = (&sim.world, sim.frac());
    let u = w.services.units.get(uid)?;
    let tick = w.clock.tick;
    let scene = engine_scene(w, uid, frac, rules, &sim.building_tops);
    let crew: Vec<Value> = w
        .unit_crew(uid)
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let fig = scene.as_ref().and_then(|s| s.figures.get(i));
            json!({
                "citizen": key_str(c), "name": w.citizens.get(c).map(|x| x.name.as_str()),
                "role": fig.map(|f| f.role.label()), "out": fig.map(|f| f.out), "pos": fig.filter(|f| f.out).map(|f| p3(f.pos)),
                "pose": fig.filter(|f| f.out).map(|f| format!("{:?}", f.pose).to_lowercase()),
            })
        })
        .collect();
    let inc = unit_incident(w, uid).map(|i| {
        let mut v = json!({
            "id": i.id, "kind": i.kind.label(), "stage": i.stage.label(), "place": i.place, "building": key_str(i.building), "name": building_name(w, Some(i.building)),
            "minutes_on_scene": i.arrived.map(|a| i.resolved.unwrap_or(tick).saturating_sub(a)), "outcome": i.outcome.map(|o| o.label()),
        });
        match i.kind {
            IncidentKind::Fire => {
                v["intensity"] = json!(round(i.intensity));
                v["how_bad"] = json!(fire_word(if i.active() { i.intensity } else { 0.0 }));
                v["minutes_to_out"] = json!(i.active().then(|| minutes_to_out(i.intensity)));
                v["progress"] = json!(fire_progress(w, i).map(round));
                v["damage"] = json!(i.damage.iter().map(|d| json!({ "name": d.name, "lost": round(d.lost), "destroyed": d.destroyed })).collect::<Vec<_>>());
            }
            _ => {
                v["patient"] = json!(i.citizen.and_then(|c| w.citizens.get(c)).map(|c| c.name.as_str()));
                v["suspect"] = json!(i.suspect.and_then(|c| w.citizens.get(c)).map(|c| c.name.as_str()));
                v["people"] = json!(i.people.iter().map(|p| json!({ "citizen": key_str(p.citizen), "name": p.name, "role": p.role, "taken_to": p.taken_to })).collect::<Vec<_>>());
            }
        }
        v
    });
    let pose = unit_pose(w, uid, frac);
    let mut v = json!({
        "unit": key_str(uid), "title": unit_title(w, uid), "kind": u.kind.label(), "call_sign": format!("{} {}", u.kind.call_sign(), u.number),
        "station": key_str(u.station), "station_name": building_name(w, Some(u.station)),
        "state": u.state.label(), "doing": unit_doing(w, uid), "lights": u.lights(), "packing": u.packing(),
        "car": u.car.filter(|c| w.traffic.cars.contains_key(*c)).map(key_str),
        "pos": pose.map(|p| v2(p.0)), "heading": pose.map(|p| round(p.1)),
        "crew_size": u.kind.crew(), "crew": crew, "incident": inc,
    });
    if let Some(s) = &scene {
        v["scene"] = json!({
            "incident": s.incident, "building": s.building.map(key_str),
            "seconds": round(s.secs),
            "water_litres": round(s.litres),
            "aim": p3(s.aim),
            "ladder": { "yaw_deg": round(s.ladder.yaw_rel.to_degrees()), "pitch_deg": round(s.ladder.pitch.to_degrees()), "length": round(s.ladder.length), "outriggers": round(s.ladder.outriggers), "tip": p3(s.tip) },
            "hoses": s.hoses.len(),
            "streams": s.streams.iter().map(|st| json!({ "from": p3(st.from), "to": p3(st.to), "flow": round(st.flow) })).collect::<Vec<_>>(),
        });
    }
    if u.kind == UnitKind::Ambulance {
        v["hospital"] = building_name(w, u.hospital);
    }
    Some(v)
}

/// `units {kind?, out?}`: every unit with where it is and what it is doing.
pub fn units_list(w: &World, kind: Option<&str>, out_only: bool, frac: f32) -> Value {
    let rows: Vec<Value> = w
        .services
        .units
        .iter()
        .filter(|(_, u)| kind.is_none_or(|k| u.kind.label().replace(' ', "_") == k.replace(' ', "_") || u.kind.call_sign().eq_ignore_ascii_case(k)))
        .filter(|(_, u)| !out_only || u.out())
        .map(|(id, u)| {
            json!({ "unit": key_str(id), "title": unit_title(w, id), "kind": u.kind.label(), "state": u.state.label(), "doing": unit_doing(w, id), "pos": unit_pose(w, id, frac).map(|p| v2(p.0)), "incident": u.incident })
        })
        .collect();
    json!({ "units": rows })
}
