//! [land] The land market over the remote API: `inspect` a building's
//! owner, price and plots; `ownership` for the selection highlight (every
//! property its owner holds, with outlines); `estate` for the pressure
//! grid, the market's counters, the billionaires and their convoys, plus
//! dev actions to force what is otherwise rare (a billionaire, a firm's
//! expansion, a buyout, sending someone on a drive).

use super::view::{field, key_from, key_str, round, v2, vec2_of};
use nd_sim::estate::{Form, OwnerKind};
use nd_sim::{BuildingId, World};
use serde_json::{json, Value};

fn owner_kind(k: &OwnerKind) -> Value {
    match k {
        OwnerKind::Firm { kind } => json!({ "kind": "firm", "business": kind.label() }),
        OwnerKind::Developer => json!({ "kind": "developer" }),
        OwnerKind::Household { household } => json!({ "kind": "household", "household": key_str(*household) }),
        OwnerKind::Elite { citizen } => json!({ "kind": "elite", "citizen": key_str(*citizen) }),
        OwnerKind::Person { citizen } => json!({ "kind": "person", "citizen": key_str(*citizen) }),
        OwnerKind::MegaCorp { kind } => json!({ "kind": "mega_corp", "origin": kind.label() }),
        OwnerKind::University { campus } => json!({ "kind": "university", "campus": key_str(*campus) }),
        OwnerKind::Company { founder } => json!({ "kind": "company", "founder": key_str(*founder) }),
        OwnerKind::Builder { founder } => json!({ "kind": "builder", "founder": key_str(*founder) }),
    }
}

fn form_key(f: Form) -> &'static str {
    match f {
        Form::McMansion => "mcmansion",
        Form::Mansion => "mansion",
        Form::Merged => "merged",
        Form::MegaHotel => "mega_hotel",
        Form::Museum => "museum",
        Form::Tower => "tower",
        Form::StripMall => "strip_mall",
        Form::Mall => "mall",
        Form::MegaMall => "mega_mall",
        Form::MegaBlock => "mega_block",
    }
}

/// `inspect.building.land`: owner, sale price, plots and form, and what
/// the pressure on its lot would let be built there.
pub fn building_land(w: &World, id: BuildingId) -> Value {
    let Some(b) = w.buildings.get(id) else { return Value::Null };
    let owner = match w.owner_of(id) {
        Some((oid, o)) => json!({ "id": key_str(oid), "name": o.name, "properties": o.properties.len(), "spent": o.spent.round(), "detail": owner_kind(&o.kind) }),
        None => json!({ "kind": "independent", "name": "independent owner" }),
    };
    json!({
        "owner": owner,
        "sale_price": w.sale_price(id),
        "lots": b.plot.lots(b.lot).iter().map(|l| key_str(*l)).collect::<Vec<_>>(),
        "merged": b.plot.merged(),
        "pending_lots": b.plot.pending_lots.iter().map(|l| key_str(*l)).collect::<Vec<_>>(),
        "form": b.plot.form.map(form_key),
        "pressure": round(w.lots.get(b.lot).map_or(0.0, |l| w.estate.pressure.at(l.center))),
        "land_density": round(w.lots.get(b.lot).map_or(0.0, |l| w.land_density_at(l.center))),
        "pressure_floors": w.pressure_floors(b.lot),
        "height_cap": w.height_cap(b.lot),
        "wants_to_expand": w.wants_to_expand(id),
        "owned_with": w.owned_with(id).iter().map(|p| key_str(*p)).collect::<Vec<_>>(),
        // [p2-look] A mall's shops and draw, a joined site's size.
        "mall": mall_json(w, id),
        "site": (!b.plot.extra_lots.is_empty()).then(|| json!({
            "plots": 1 + b.plot.extra_lots.len(),
            "backland": b.plot.extra_lots.iter().filter(|l| w.lots.get(**l).is_some_and(|x| x.landlocked)).count(),
            "area_m2": round(w.footprint(id).map_or(0.0, |l| l.area())),
        })),
    })
}

/// [p2-look] `inspect.building.land.mall` of a strip mall, mall or
/// mega-mall: its shop units by role (let or to let, who has them), the
/// car park and the reach; null for anything else.
fn mall_json(w: &World, id: BuildingId) -> Value {
    let Some(b) = w.buildings.get(id).filter(|b| b.plot.form.is_some_and(|f| f.is_mall())) else { return Value::Null };
    let units: Vec<Value> = b
        .units
        .iter()
        .filter(|u| u.shopfront)
        .map(|u| {
            let who = match u.occupant {
                nd_sim::Occupant::Vacant => Value::Null,
                nd_sim::Occupant::Main => json!({ "id": key_str(id), "name": b.name, "own": true }),
                nd_sim::Occupant::Tenant(t) => json!({ "id": key_str(t), "name": w.buildings.get(t).map(|x| x.name.clone()), "kind": w.buildings.get(t).and_then(|x| x.business.as_ref()).map(|z| z.kind.label()) }),
            };
            json!({ "role": format!("{:?}", u.role).to_lowercase(), "floor": u.lo, "let": !u.vacant(), "occupant": who })
        })
        .collect();
    json!({
        "shops": units.len(),
        "let": b.units.iter().filter(|u| u.shopfront && !u.vacant()).count(),
        "tenants": w.tenants_of(id).len(),
        "parking_spaces": b.parking_capacity,
        "parking_used": b.parking_used,
        "reach": round(w.shop_reach(id)),
        "reach_m": round(w.shopper_reach_m(id)),
        "units": units,
    })
}

/// `ownership {building}`: what to highlight when it is selected: the
/// owner and every property, each with its name, centre and outline (the
/// merged footprint for one over several plots); a billionaire's homes are
/// flagged.
pub fn ownership(cmd: &Value, w: &World) -> Result<Value, String> {
    let id: BuildingId = key_from(field(cmd, "building").ok_or("ownership needs a building")?)?;
    if !w.buildings.contains_key(id) {
        return Err("no such building".into());
    }
    let props: Vec<Value> = w
        .owned_with(id)
        .into_iter()
        .filter_map(|p| {
            let b = w.buildings.get(p)?;
            let fp = w.footprint(p)?;
            Some(json!({
                "id": key_str(p), "name": b.name, "land_use": b.land_use.label(), "pos": v2(fp.center),
                "outline": fp.polygon.iter().map(|q| v2(*q)).collect::<Vec<_>>(),
                "home": b.homes > 0, "business": b.business.as_ref().map(|z| z.kind.label()),
                "form": b.plot.form.map(form_key), "selected": p == id,
            }))
        })
        .collect();
    let owner = w.owner_of(id).map(|(oid, o)| json!({ "id": key_str(oid), "name": o.name, "detail": owner_kind(&o.kind) }));
    Ok(json!({ "building": key_str(id), "owner": owner, "highlight": props }))
}

fn elite_json(w: &World, e: &nd_sim::estate::Elite) -> Value {
    let c = w.citizens.get(e.citizen);
    let convoy = w.estate.convoys.iter().find(|v| v.citizen == e.citizen).map(|v| {
        json!({
            "boss_car": key_str(v.boss), "boss_on_road": w.traffic.cars.contains_key(v.boss),
            "escorts": v.escorts.iter().map(|x| json!({ "car": key_str(*x), "pos": w.traffic.cars.get(*x).map(|c| v2(c.pos)) })).collect::<Vec<_>>(),
            "police": v.police.map(key_str),
            "boss_pos": w.traffic.cars.get(v.boss).map(|c| v2(c.pos)),
            "guards_at": v.stop.map(|(p, _)| v2(p)),
        })
    });
    json!({
        "citizen": key_str(e.citizen), "name": c.map(|c| c.name.clone()), "net_worth": e.net_worth.round(),
        "origin": match e.origin { nd_sim::estate::EliteOrigin::Local => "local", nd_sim::estate::EliteOrigin::Visitor => "visitor" },
        "mansion": e.mansion.map(key_str), "mansion_pos": e.mansion.and_then(|m| w.footprint(m)).map(|l| v2(l.center)),
        "home": c.and_then(|c| c.home).map(key_str), "pos": c.map(|c| v2(c.pos)),
        "activity": c.map(|c| format!("{:?}", c.activity).split([' ', '{', '(']).next().unwrap_or("").to_string()),
        "cars": e.cars.len(), "escorts": e.escorts, "police_escort": e.police, "guards": e.guards(),
        "properties": w.estate.owners.get(e.owner).map_or(0, |o| o.properties.len()),
        "convoy": convoy,
    })
}

/// `estate`: the market's state, or a dev action.
pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let action = field(cmd, "action").and_then(|v| v.as_str()).unwrap_or("summary");
    match action {
        "summary" => Ok(summary(w)),
        "pressure" => {
            if let Some(p) = field(cmd, "at") {
                let p = vec2_of(p)?;
                let lot = w.lots.iter().find(|(_, l)| l.contains(p)).map(|(id, _)| id);
                return Ok(json!({ "at": v2(p), "pressure": round(w.estate.pressure.at(p)), "land_density": round(w.land_density_at(p)), "foot_traffic": round(w.estate.pressure.foot_at(p)), "mobility": round(w.estate.pressure.mobility_at(p)), "floors": w.estate.pressure.floors_at(p), "lot_height_cap": lot.map(|l| w.height_cap(l)) }));
            }
            let p = &w.estate.pressure;
            let cells: Vec<Value> = (0..p.value.len()).filter(|i| p.value[*i] >= 0.5).map(|i| json!([v2(p.cell_center(i)), round(p.value[i]), round(p.mobility[i])])).collect();
            Ok(json!({ "cell": nd_sim::estate::PRESSURE_CELL, "samples": p.samples, "cells": cells }))
        }
        "spawn_elite" => {
            let mut rng = nd_sim::rng_for(w.config.seed ^ 0xE11E, w.clock.tick);
            let c = w.spawn_elite(&mut rng)?;
            let e = w.estate.elite_of(c).cloned().ok_or("not recorded")?;
            Ok(json!({ "elite": elite_json(w, &e) }))
        }
        "expand" => {
            let id: BuildingId = key_from(field(cmd, "building").ok_or("expand needs a building")?)?;
            let kind = w.buildings.get(id).and_then(|b| b.business.as_ref()).map(|z| z.kind).ok_or("no business there")?;
            let grew = if nd_sim::estate::branches(kind) {
                w.open_branch(id).map(|b| json!({ "branch": key_str(b) }))
            } else {
                w.expand_into_neighbours(id).then(|| json!({ "merged": true, "pending_lots": w.buildings[id].plot.pending_lots.iter().map(|l| key_str(*l)).collect::<Vec<_>>(), "lots": w.buildings[id].plot.lots(w.buildings[id].lot).iter().map(|l| key_str(*l)).collect::<Vec<_>>() }))
            };
            grew.ok_or_else(|| "nowhere to grow".to_string())
        }
        "buyout" => {
            let id: BuildingId = key_from(field(cmd, "building").ok_or("buyout needs a building")?)?;
            let mut rng = nd_sim::rng_for(w.config.seed ^ 0xB0B0, w.clock.tick);
            if w.buy_out_and_rebuild(id, &mut rng) {
                Ok(json!({ "bought": key_str(id), "owner": w.owner_of(id).map(|(_, o)| o.name.clone()) }))
            } else {
                Err("not for sale (or not worth more rebuilt)".into())
            }
        }
        "drive" => {
            let c = key_from(field(cmd, "citizen").ok_or("drive needs a citizen")?)?;
            let to = key_from(field(cmd, "to").ok_or("drive needs a building to go to")?)?;
            Ok(json!({ "sent": w.send_citizen(c, to) }))
        }
        other => Err(format!("unknown estate action '{other}' (summary, pressure, spawn_elite, expand, buyout, drive)")),
    }
}

fn summary(w: &World) -> Value {
    let s = &w.estate.stats;
    let (pop, gini, ratio, ok) = w.elite_gate();
    let mut floors = [0u32; 6];
    for b in w.buildings.values().filter(|b| b.construction.is_none() && !b.is_special()) {
        floors[match b.floors {
            0..=1 => 0,
            2 => 1,
            3..=4 => 2,
            5..=8 => 3,
            9..=16 => 4,
            _ => 5,
        }] += 1;
    }
    let forms: Vec<Value> = w.buildings.iter().filter_map(|(id, b)| b.plot.form.map(|f| json!({ "id": key_str(id), "name": b.name, "form": form_key(f), "lots": 1 + b.plot.extra_lots.len(), "pending": b.plot.pending_lots.len(), "pos": w.footprint(id).map(|l| v2(l.center)), "under_construction": b.construction.is_some() }))).collect();
    let owners: Vec<Value> = w.estate.owners.iter().map(|(id, o)| json!({ "id": key_str(id), "name": o.name, "detail": owner_kind(&o.kind), "properties": o.properties.iter().map(|p| key_str(*p)).collect::<Vec<_>>() })).collect();
    json!({
        "stats": {
            "buyouts_total": s.buyouts_total, "buyouts_today": s.buyouts_today, "merges_total": s.merges_total, "branches_total": s.branches_total,
            "mcmansions_total": s.mcmansions_total, "mansions_total": s.mansions_total, "elites_total": s.elites_total,
            "priced_out_total": s.priced_out_total, "convoys_total": s.convoys_total,
            "relocated_total": s.relocated_total, "relocate_failed_total": s.relocate_failed_total, "investments_total": s.investments_total,
            "parking_opened_total": w.economy.parking_opened_total,
        },
        "moving": w.estate.moving.iter().map(|m| json!({ "name": m.name, "kind": m.business.kind.label(), "why": m.why, "from": v2(m.from), "money": round(m.business.money) })).collect::<Vec<_>>(),
        "pressure_samples": w.estate.pressure.samples,
        "floors_histogram": { "1": floors[0], "2": floors[1], "3-4": floors[2], "5-8": floors[3], "9-16": floors[4], "17+": floors[5] },
        "elite_gate": { "population": pop, "gini": round(gini), "top_over_median": round(ratio), "open": ok, "cap": w.elite_cap(),
            "needs": format!("population >= {}, Gini >= {}, top 5% earning >= {}x the median", nd_sim::estate::ELITE_MIN_POP, nd_sim::estate::ELITE_MIN_GINI, nd_sim::estate::ELITE_MIN_TOP_RATIO) },
        "elites": w.estate.elites.iter().map(|e| elite_json(w, e)).collect::<Vec<_>>(),
        "nimby": w.estate.nimby.iter().map(|n| json!({ "center": v2(n.center), "radius": round(n.radius), "strength": round(n.strength), "mansion": key_str(n.mansion), "price_out_left": n.price_out_left })).collect::<Vec<_>>(),
        "forms": forms,
        "owners": owners,
        "reserved_lots": w.estate.reserved.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::{Business, BusinessKind, LandUse, Vec2, WorldConfig};

    fn rect(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
    }

    /// A pizza chain with two shops at opposite ends of town: selecting
    /// either one, `ownership` hands back both, with their outlines, for
    /// the highlight; `inspect`'s land section names the owner and price.
    #[test]
    fn ownership_highlights_everything_the_owner_holds() {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: nd_sim::LandscapeParams::flat(), ..WorldConfig::default() });
        w.add_road(&[Vec2::new(-900.0, 0.0), Vec2::new(900.0, 0.0)], 1, 1);
        w.add_zone(rect(-850.0, 8.0, 120.0, 60.0), LandUse::Commercial);
        w.add_zone(rect(730.0, 8.0, 120.0, 60.0), LandUse::Commercial);
        let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use == LandUse::Commercial && l.center.x < 0.0).map(|(id, _)| id).next().unwrap();
        w.build_on(lot);
        let shop = w.lots[lot].building.unwrap();
        w.finish_construction(shop);
        let jobs = w.buildings[shop].jobs;
        w.buildings[shop].business = Some(Business::new(BusinessKind::Bakery, jobs));
        let branch = w.open_branch(shop).expect("a branch across town");
        for (picked, other) in [(shop, branch), (branch, shop)] {
            let v = ownership(&json!({ "building": key_str(picked) }), &w).unwrap();
            let ids: Vec<String> = v["highlight"].as_array().unwrap().iter().map(|h| h["id"].as_str().unwrap().to_string()).collect();
            assert!(ids.contains(&key_str(picked)) && ids.contains(&key_str(other)), "{ids:?}");
            for h in v["highlight"].as_array().unwrap() {
                assert!(h["outline"].as_array().unwrap().len() >= 3, "each with its outline");
            }
            assert!(v["owner"]["name"].is_string());
        }
        let land = building_land(&w, shop);
        assert!(land["sale_price"].as_f64().unwrap() > 0.0);
        assert_eq!(land["owned_with"].as_array().unwrap().len(), 2);
        assert_eq!(land["owner"]["detail"]["kind"], "firm");
        // An independent building highlights only itself.
        let lone = w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use == LandUse::Commercial && l.building.is_none()).map(|(id, _)| id).next().unwrap();
        w.build_on(lone);
        let b = w.lots[lone].building.unwrap();
        let v = ownership(&json!({ "building": key_str(b) }), &w).unwrap();
        assert_eq!(v["highlight"].as_array().unwrap().len(), 1);
        assert!(v["owner"].is_null());
    }
}
