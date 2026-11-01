//! [society] Crime, homelessness and trash for the remote API: the
//! `society` report section, its alerts (crime hot spots, no jail, growing
//! camps, overflowing trash) and the `society` command, which reads the
//! same and has a few levers for trying the systems out (evict a
//! building's households, make a citizen offend, fill the bins).

use nd_sim::crime::{Offence, HOTSPOT_HEAT};
use nd_sim::trash::{Hauler, COMPANY};
use nd_sim::{ServiceKind, World};
use serde_json::{json, Map, Value};

use super::report::{building_pos, Alert};
use super::view::{building_name, field, key_from, key_str, opt_f32, opt_str, req_str, round, v2};

fn by_kind(counts: &[u32; 5]) -> Value {
    let mut m = Map::new();
    for o in Offence::ALL {
        m.insert(o.key().into(), json!(counts[o.index()]));
    }
    m.insert("total".into(), json!(counts.iter().sum::<u32>()));
    Value::Object(m)
}

/// [r5-trash] Where the trash goes: tonnes and costs by way, the sites,
/// the depots' yards, the starter contract, litter.
pub fn disposal(w: &World) -> Value {
    use nd_sim::waste::Dest;
    let t = &w.society.trash;
    let d = &t.disposal;
    let by = |a: &[f32; Dest::COUNT]| {
        let mut m = Map::new();
        for k in Dest::ALL {
            m.insert(k.key().into(), json!(round(a[k.index()])));
        }
        Value::Object(m)
    };
    let sites: Vec<Value> = w
        .waste_sites()
        .iter()
        .map(|s| {
            let st = d.sites.get(&s.id).cloned().unwrap_or_default();
            json!({ "id": key_str(s.id), "name": building_name(w, Some(s.id)), "kind": s.kind.key(), "tier": format!("{:?}", s.tier).to_lowercase(), "pos": v2(s.pos),
                "staffing": round(s.staffing), "capacity_t": round(nd_sim::waste::site_capacity(s.kind, s.tier)), "room_t": round(w.site_room_t(s)),
                "filled_t": round(st.filled_t), "fill": w.landfill_fill(s.id).map(round), "backlog_t": round(st.backlog_t),
                "in_today_t": round(st.in_today_t), "in_yesterday_t": round(st.in_yesterday_t), "done_yesterday_t": round(st.done_yesterday_t),
                "recovered_yesterday_t": round(st.recovered_yesterday_t), "mw": round(st.mw), "mwh_yesterday": round(st.mwh_yesterday),
                "cost_yesterday": round(st.cost_yesterday), "sales_yesterday": round(st.sales_yesterday), "turned_away_yesterday": st.turned_away_yesterday })
        })
        .collect();
    let yards: Vec<Value> = d.yards.iter().map(|(id, tt)| json!({ "depot": key_str(*id), "name": building_name(w, Some(*id)), "held_t": round(*tt) })).collect();
    let trucks_dumping = t.trucks.values().filter(|x| x.dump.is_some()).count();
    let contractor = t.trucks.values().filter(|x| matches!(x.hauler, Hauler::Contractor(_))).count();
    json!({
        "unit": "t", "starter_contract": d.starter_on(w.clock.day()), "starter_until_day": d.starter_until, "road_price_per_t": round(w.road_price()),
        "today_t": by(&d.t_today), "yesterday_t": by(&d.t_yesterday), "total_t": by(&d.t_total),
        "cost_today": by(&d.cost_today), "cost_yesterday": by(&d.cost_yesterday), "cost_total": round(d.cost_total),
        "sales_yesterday": round(d.sales_yesterday), "sales_total": round(d.sales_total),
        "held_in_yards_t": round(d.held_t()), "yards": yards, "blocked_loads_yesterday": d.blocked_yesterday, "yard_full_days": d.yard_full_days,
        "waste_mw": round(w.waste_mw()), "ships_by_rail": w.waste_ships_by_rail(), "sites": sites,
        "trucks_to_sites": trucks_dumping, "contractor_lorries": contractor,
        "littered_streets": t.litter.values().filter(|l| **l >= 0.25).count(), "litter_total": round(t.litter.values().sum::<f32>()),
        "scavengers_last_night": d.scavengers_yesterday, "scavenged_total": round(d.scavenged_total), "landfill_illness_total": d.landfill_ill_total,
    })
}

/// The `society` report section.
pub fn society(w: &World) -> Value {
    let s = &w.society;
    let c = &s.crime;
    let h = &s.homeless;
    let t = &s.trash;
    let mut hot: Vec<_> = c.hotspots.iter().collect();
    hot.sort_by(|a, b| b.heat.total_cmp(&a.heat));
    let in_custody = c.offenders.iter().filter(|o| o.in_custody).count();
    let depots: Vec<Value> = w
        .trash_depots()
        .iter()
        .map(|(id, _, r, crewed)| {
            json!({ "id": key_str(*id), "name": building_name(w, Some(*id)), "radius_m": round(*r), "trucks_crewed": crewed,
                "trucks_out": t.trucks.values().filter(|x| x.hauler == Hauler::City(*id)).count() })
        })
        .collect();
    let trucks_city = t.trucks.values().filter(|x| matches!(x.hauler, Hauler::City(_))).count();
    let trucks_private = t.trucks.values().filter(|x| x.hauler == Hauler::Private).count();
    json!({
        "criminals": c.offenders.len(),
        "crimes_today": by_kind(&c.stats.offences_today),
        "arrests": c.stats.arrests_total,
        "reoffend_rate": round(c.reoffend_rate()),
        "homeless": w.homeless_people(),
        "camps": h.camps.len(),
        "rvs": h.rvs.len(),
        "trash": {
            "produced": round(t.stats.produced_today), "collected_city": round(t.stats.collected_city_today), "collected_private": round(t.stats.collected_private_today),
            "overflowing_buildings": t.stats.overflowing_buildings,
            "produced_yesterday": round(t.stats.produced_yesterday), "collected_city_yesterday": round(t.stats.collected_city_yesterday), "collected_private_yesterday": round(t.stats.collected_private_yesterday),
            "unit": "kg", "stored_kg": round(t.stored()), "covered_buildings": t.stats.covered_buildings, "buildings_with_bins": t.bins.len(),
            "stops_today": t.stats.stops_today, "complaints_yesterday": t.stats.complaints_yesterday, "rat_illness_total": t.stats.rat_illness_total,
            "trucks": { "city": trucks_city, "private": trucks_private, "on_road": t.trucks.values().filter(|x| x.car.is_some()).count() },
            "depots": depots,
            "private_hauler": { "company": COMPANY, "clients": t.stats.private_clients, "revenue_yesterday": round(t.stats.private_revenue_yesterday), "revenue_total": round(t.stats.private_revenue_total) },
            // [r5-trash] where it goes
            "disposal": disposal(w),
        },
        "crime": {
            "criminals_free": c.offenders.len() - in_custody, "in_custody": in_custody,
            "crimes_yesterday": by_kind(&c.stats.offences_yesterday), "crimes_total": by_kind(&c.stats.offences_total),
            "arrests_today": c.stats.arrests_today, "jailed_total": c.stats.jailed_total,
            "released_total": c.stats.released_total, "released_early_total": c.stats.released_early_total, "reoffended_total": c.stats.reoffended_total,
            "reformed_total": c.stats.reformed_total, "recruited_total": c.stats.recruited_total, "killed_total": c.stats.killed_total,
            "stolen_today": round(c.stats.stolen_today), "stolen_total": round(c.stats.stolen_total), "fled_crime_total": c.stats.fled_crime_total,
            "hotspots": hot.iter().take(8).map(|x| json!({ "pos": v2(x.pos), "heat": round(x.heat) })).collect::<Vec<_>>(),
            "jails": w.buildings.values().filter(|b| b.service.is_some_and(|s| s.kind == ServiceKind::Jail)).count(),
        },
        "street": {
            "households": h.households(), "sleeping_rough": h.stats.sleeping_rough,
            "evicted_today": h.stats.evicted_today, "evicted_total": h.stats.evicted_total, "rehoused_today": h.stats.rehoused_today, "rehoused_total": h.stats.rehoused_total,
            "outreach_total": h.stats.outreach_total, "left_town_total": h.stats.left_town_total, "meals_today": h.stats.meals_today,
            "camps_at_last_midnights": h.stats.camps_history,
            "camp_list": h.camps.iter().map(|x| json!({ "id": x.id, "pos": v2(x.pos), "people": x.people, "tents": x.tents(), "households": x.households.len(), "on": if x.sidewalk { "pavement" } else { "vacant lot" } })).collect::<Vec<_>>(),
            "rv_list": h.rvs.iter().map(|r| json!({ "id": r.seed, "pos": v2(r.pos), "kind": if r.camper { "rv" } else { "van" }, "abandoned": r.abandoned.is_some() })).collect::<Vec<_>>(),
        },
    })
}

/// Crime hot spots, criminals with no jail, growing camps, overflowing trash.
pub fn alerts(w: &World) -> Vec<Alert> {
    let s = &w.society;
    let mut out = Vec::new();
    let mut hot = Alert::new("crime_hotspot", 2, "offences pile up here: police coverage (Buildings > Services) deters and arrests; jobs and housing nearby stop people turning to crime");
    let mut hottest = 0.0f32;
    for h in s.crime.hotspots.iter().filter(|h| h.heat >= HOTSPOT_HEAT) {
        hot.add(format!("[{:.0}, {:.0}]", h.pos.x, h.pos.y), Some(h.pos));
        hottest = hottest.max(h.heat);
    }
    hot.message = format!("{} crime hot spots (heat up to {:.1}); land value falls and the better-off move away", hot.count, hottest);
    out.push(hot);
    let jails = w.buildings.values().any(|b| b.service.is_some_and(|x| x.kind == ServiceKind::Jail));
    if !jails && s.crime.offenders.len() >= 5 {
        let mut a = Alert::new("no_jail", 2, "place a jail (Buildings > Services): a sentence ends more criminal careers; without one arrests are let go after a few hours and offend again");
        for o in s.crime.offenders.iter().take(200) {
            a.add(key_str(o.citizen), w.citizens.get(o.citizen).map(|c| c.pos));
        }
        a.message = format!("{} criminals in town and no jail ({} let go early so far)", s.crime.offenders.len(), s.crime.stats.released_early_total);
        out.push(a);
    }
    let people = w.homeless_people();
    if s.homeless.growing() || people >= 10 {
        let mut a = Alert::new("growing_camps", 2, "people evicted with nothing affordable camp on cheap land: zone residential where land is cheap (low-tier homes house them again); a clinic or police post nearby runs outreach");
        for c in &s.homeless.camps {
            a.add(format!("camp {}", c.id), Some(c.pos));
        }
        a.count = a.count.max(1);
        a.message = format!("{} people live on the street: {} camps, {} in vehicles{}", people, s.homeless.camps.len(), s.homeless.rvs.len(), if s.homeless.growing() { ", and the camps are growing" } else { "" });
        out.push(a);
    }
    let bins = &s.trash.bins;
    let over: Vec<_> = bins.iter().filter(|(_, b)| b.overflow() > 0.0).collect();
    if !over.is_empty() {
        let share = over.len() as f32 / bins.len().max(1) as f32;
        let mut a = Alert::new("overflowing_trash", if share > 0.25 { 3 } else { 2 }, "place a trash depot (Buildings > Services) and let it hire two crew per truck; a truck makes forty to seventy stops a day, so big cities need several");
        for (id, _) in &over {
            a.add(key_str(**id), w.buildings.get(**id).and_then(|b| building_pos(w, b)));
        }
        a.message = format!("{} buildings' bins are overflowing ({:.0}% of them); residents complain and the land loses value", over.len(), share * 100.0);
        out.push(a);
    }
    // [r5-trash] Depots that cannot unload, landfills filling up.
    let d = &s.trash.disposal;
    if d.blocked_yesterday > 0 {
        let mut a = Alert::new("trash_yards_full", 3, "build a landfill, incinerator, recycling plant or transfer station within 3 km of the depots (Buildings > Services > Sanitation), or get the treasury out of the red so the contractor hauls in full");
        for (id, _) in d.yards.iter().filter(|(_, t)| **t > 0.0) {
            a.add(key_str(*id), w.buildings.get(*id).and_then(|b| building_pos(w, b)));
        }
        a.message = format!("{} truckloads could not be unloaded yesterday: the depots' yards are full, so the trucks stop collecting", d.blocked_yesterday);
        out.push(a);
    }
    let filling: Vec<_> = w.waste_sites().into_iter().filter(|s| s.kind == ServiceKind::Landfill && w.landfill_fill(s.id).is_some_and(|f| f >= 0.85)).collect();
    if !filling.is_empty() {
        let mut a = Alert::new("landfill_nearly_full", 2, "build another landfill, or burn, sort or ship more (an incinerator, a recycling plant, a transfer station)");
        for s in &filling {
            a.add(key_str(s.id), Some(s.pos));
        }
        a.message = format!("{} landfill{} 85% full or more", filling.len(), if filling.len() == 1 { " is" } else { "s are" });
        out.push(a);
    }
    out
}

/// `society {action?}`: no action (or `report`) returns the section;
/// `evict {building}` puts its households on the street; `offend {citizen,
/// building?, kind?}` makes a citizen a criminal and has them strike;
/// `trash {kg, building?}` adds trash to one building's bins or everyone's.
pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let action = opt_str(cmd, "action")?.unwrap_or("report");
    match action {
        "report" | "status" => Ok(society(w)),
        "evict" => {
            let b: nd_sim::BuildingId = key_from(field(cmd, "building").ok_or("building is required")?)?;
            let hhs = w.buildings.get(b).ok_or("no such building")?.households.clone();
            let n = hhs.into_iter().filter(|h| w.force_homeless(*h)).count();
            Ok(json!({ "evicted_households": n, "homeless": w.homeless_people(), "camps": w.society.homeless.camps.len(), "rvs": w.society.homeless.rvs.len() }))
        }
        "offend" => {
            let c: nd_sim::CitizenId = key_from(field(cmd, "citizen").ok_or("citizen is required")?)?;
            if !w.citizens.contains_key(c) {
                return Err("no such citizen".into());
            }
            w.turn_to_crime(c);
            let at = match field(cmd, "building") {
                Some(v) => Some(key_from(v)?),
                None => None,
            };
            let id = match opt_str(cmd, "kind")? {
                Some(k) => {
                    let o = Offence::ALL.into_iter().find(|o| o.key() == k).ok_or_else(|| format!("unknown kind {k:?}; one of theft, burglary, assault, murder, vandalism"))?;
                    let b = at.ok_or("a kind needs a building")?;
                    w.offend(c, b, o)
                }
                None => w.commit_offence(c, at),
            };
            Ok(json!({ "incident": id, "criminals": w.society.crime.offenders.len() }))
        }
        // [feat/pt-ux] Have a camper or van towed, as its inspector's button.
        "tow_rv" => {
            let v = field(cmd, "rv").ok_or("rv (its id, `rv_list`) is required")?;
            let seed = v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).ok_or("rv: an id (a number)")?;
            w.order_tow_rv(seed)?;
            Ok(json!({ "rv": seed, "tow_at": w.society.homeless.rv(seed).and_then(|r| r.tow_at) }))
        }
        "abandon_rv" => {
            let v = field(cmd, "rv").ok_or("rv (its id, `rv_list`) is required")?;
            let seed = v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).ok_or("rv: an id (a number)")?;
            w.abandon_rv(seed)?;
            Ok(json!({ "rv": seed, "tow_at": w.society.homeless.rv(seed).and_then(|r| r.tow_at) }))
        }
        "trash" => {
            let kg = opt_f32(cmd, "kg")?.unwrap_or(100.0);
            let only = match field(cmd, "building") {
                Some(v) => Some(key_from::<nd_sim::BuildingId>(v)?),
                None => None,
            };
            let mut n = 0;
            for (id, bin) in w.society.trash.bins.iter_mut() {
                if only.is_none_or(|b| b == *id) {
                    bin.stored += kg;
                    n += 1;
                }
            }
            Ok(json!({ "bins": n, "overflowing": w.society.trash.bins.values().filter(|b| b.overflow() > 0.0).count() }))
        }
        _ => {
            let _ = req_str(cmd, "action")?;
            Err(format!("unknown society action {action:?}; one of report, evict, offend, tow_rv, abandon_rv, trash"))
        }
    }
}
