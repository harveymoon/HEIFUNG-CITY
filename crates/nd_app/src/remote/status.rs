//! `status`, `memory` and the clock.

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use super::{camera, identity, land, look, planning, report, tool};
use crate::sim::{Sim, SPEEDS};

pub(super) fn clock(sim: &Sim) -> Value {
    let c = &sim.world.clock;
    json!({ "day": c.day() + 1, "time": c.format_hhmm(), "tick": c.tick, "hour": c.hour(), "minute": c.minute() })
}

/// [memory] The process's working set (and its peak) and commit charge,
/// the renderer's asset and entity counts, and the world's big collections.
pub(super) fn memory_json(ctx: &Ctx) -> Value {
    let (ws, peak_ws, commit) = crate::memory::process_memory();
    let heap = crate::memory::heap();
    let mb = |n: usize| round(n as f32 / 1_048_576.0);
    let w = &ctx.sim.world;
    json!({
        "working_set_mb": mb(ws), "peak_working_set_mb": mb(peak_ws), "commit_mb": mb(commit),
        "heap": { "note": "with the heap-count feature only", "live_mb": mb(heap.0), "peak_mb": mb(heap.1), "biggest_block_mb": mb(heap.2) },
        "meshes": ctx.meshes.len(), "materials": ctx.materials.len(), "images": ctx.images.len(), "entities": ctx.entities.iter().count(),
        "render": { "building_entities": ctx.stats.building_entities, "people_meshes": ctx.stats.people_meshes },
        "world": { "citizens": w.citizens.len(), "buildings": w.buildings.len(), "cars": w.traffic.cars.len(), "lots": w.lots.len(), "day": w.clock.day() },
    })
}

pub(super) fn status(ctx: &Ctx) -> Value {
    let sim = &ctx.sim;
    let w = &sim.world;
    let e = &w.economy;
    let p = &e.power;
    let t = &e.transit;
    let r = &w.rail.stats;
    let sites = w.buildings.values().filter(|b| b.construction.is_some()).count();
    let fps = ctx.diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let ledger: serde_json::Map<String, Value> = e.ledger.yesterday_sorted().into_iter().map(|(f, x)| (f.label().to_string(), json!(round(x)))).collect();
    let power_rows = report::power_json(w, false);
    let mut v = json!({
        "clock": clock(sim), "paused": sim.paused, "speed_index": sim.speed_index, "speed": sim.speed(), "speed_label": SPEEDS[sim.speed_index].0,
        "effective_speed": round(sim.effective_speed), "ticks_last_frame": sim.ticks_last_frame,
        "population": w.citizens.len(), "buildings": w.buildings.len() - sites, "under_construction": sites,
        "lots": w.lots.len(), "lots_vacant": w.lots.values().filter(|l| l.building.is_none() && l.usable()).count(), "lots_unusable": w.lots.values().filter(|l| l.landlocked).count(),
        "homes": { "used": e.homes_total - e.homes_free, "total": e.homes_total }, "jobs": { "used": e.jobs_total - e.jobs_free, "total": e.jobs_total },
        "waiting_immigrants": e.waiting_immigrants, "unhoused": e.unhoused, "unemployed": e.unemployed,
        "treasury": round(e.treasury), "tax_today": round(e.tax_today), "ledger_yesterday": ledger,
        "power": {
            "supply_mw": round(p.supply_mw), "imported_mw": round(p.imported_mw), "exported_mw": round(p.exported_mw), "demand_mw": round(p.demand_mw), "served": round(p.served),
            "plants": p.plants, "unpowered_buildings": p.unpowered_buildings, "brownout_hours_today": p.brownout_hours_today, "upkeep_today": round(p.upkeep_today),
            "import_cost_today": round(p.import_cost_today), "export_income_today": round(p.export_income_today),
            "import_price": round(p.import_price), "export_price": round(p.export_price),
            "batteries": { "count": p.batteries, "charge_mwh": round(p.battery_charge_mwh), "capacity_mwh": round(p.battery_capacity_mwh), "charging_mw": round(p.battery_in_mw), "discharging_mw": round(p.battery_out_mw) },
            "rows": { "plants": power_rows["plants"], "batteries": power_rows["batteries"] },
        },
        "weather": land::weather_json(w),
        "land": { "avg_land_value": round(e.avg_land_value), "gini": round(e.gini), "displaced_total": e.displaced_total, "emigrated_total": e.emigrated_total, "priced_out_total": e.priced_out_total, "tier_counts": e.tier_counts, "class_counts": e.class_counts, "avg_rent": e.avg_rent.map(round) },
        "market": { "businesses": e.market.businesses, "deals_today": e.market.deals_today, "imports_today": round(e.market.imports_today), "exports_today": round(e.market.exports_today), "meals_sold_today": e.market.meals_sold_today, "hungry": e.market.hungry, "closed_total": e.market.closed_total },
        "demand": nd_sim::LandUse::ALL.iter().map(|&lu| (lu.label().to_string(), json!(round(e.demand_for(lu))))).collect::<serde_json::Map<_, _>>(),
        "traffic": { "cars": w.traffic.cars.len(), "lanes": w.traffic.net.lanes.len(), "signals": w.traffic.net.intersections.iter().filter(|i| i.signalized).count(), "parking_used": e.parking_used, "parking_capacity": e.parking_capacity, "walk_trips_today": e.walk_trips_today, "pedestrians": { "waiting_at_crossings": w.peds.waiting_now, "jaywalkers_today": w.peds.jaywalkers_today, "near_misses_today": w.peds.near_misses_today, "ped_collisions_today": w.peds.collisions_today } },
        "bots": { "active": w.bots.bots.len(), "delivered_today": w.bots.delivered_today, "vans_on_road": nd_sim::Bots::vans(w), "van_orders_today": w.bots.van_orders_today },
        "transit": { "routes": w.transit.routes.len(), "buses": t.buses, "depots": t.depots, "depot_capacity": t.depot_capacity, "waiting": t.waiting, "riding": t.riding, "riders_today": t.riders_today, "fares_today": round(t.fares_today), "upkeep_today": round(t.upkeep_today) },
        "rail": { "trains": r.trains, "stations": r.stations, "yards": r.yards, "waiting": r.waiting, "riding": r.riding, "riders_today": r.riders_today, "fares_today": round(r.fares_today), "next_passenger_in": r.next_passenger_in, "freight_in_today": round(r.freight_in_today), "freight_waiting": round(r.freight_waiting), "upkeep_today": round(r.upkeep_today) },
        "render": { "fps": round(fps as f32), "ticks_per_frame": sim.ticks_last_frame, "triangles": ctx.stats.street_triangles + ctx.stats.building_triangles, "building_meshes": ctx.stats.building_entities, "detail_triangles": { "mid": ctx.stats.detail_triangles[0], "near": ctx.stats.detail_triangles[1] }, "detail_in_range": { "mid": ctx.stats.detail_in_range[0], "near": ctx.stats.detail_in_range[1] }, "drawn_triangles": ctx.stats.street_triangles + ctx.stats.building_triangles - ctx.stats.detail_triangles.iter().sum::<usize>() + ctx.stats.detail_in_range.iter().sum::<usize>(), "graphics": look::graphics_json(ctx) },
        "camera": camera::camera_pose(ctx), "tool": tool::tool_state(&ctx.tools), "selection": selection(w, *ctx.selection),
        "project": planning::project_state(ctx)["project"], "overlays": look::overlays_json(&ctx.overlays),
        "lighting": { "follow_clock": ctx.lighting.follow_clock, "hour": round(ctx.lighting.hour), "heat_vision": ctx.lighting.heat }, "photo_mode": ctx.flags.photo_mode,
        "message": ctx.flags.message, "tool_status": ctx.tools.status, "event_seq": ctx.log.seq,
        "favourites": ctx.tools.favorites.iter().map(|&f| key_str(f)).collect::<Vec<_>>(),
    });
    // [camera] What the camera is locked on, if anything.
    v["following"] = json!(ctx.follow.target.map(|s| selection(w, s)));
    // [chase-cam] How it follows, and what the chase camera cuts away.
    v["follow_view"] = json!(ctx.follow.target.map(|_| ctx.follow.view.label()));
    v["chasing"] = json!(ctx.follow.chasing());
    // [subway] Added apart: the literal above is at serde_json's macro depth.
    let ss = &w.subway.stats;
    v["subway"] = json!({ "stations": ss.stations, "lines": ss.lines, "trains": ss.trains, "waiting": ss.waiting, "riding": ss.riding, "riders_today": ss.riders_today, "riders_yesterday": ss.riders_yesterday, "fares_today": round(ss.fares_today) });
    // Added after the literal: one `json!` this size hits the macro recursion limit.
    v["windows_open"] = json!(ctx.windows.open_names());
    v["city_data"] = ctx.windows.data_status();
    v["build_window"] = json!({ "open": ctx.windows.build_open, "section": ctx.windows.build_section });
    v["title_screen"] = json!(ctx.saves.title);
    // [climate] The map's climate and what its weather is doing to the view.
    v["climate"] = json!({ "climate": w.landscape.params.climate.key(), "label": w.landscape.params.climate.label(), "precipitation": crate::render::climate::precip_now(w).map(|(k, i)| json!({ "kind": k, "intensity": round(i) })) });
    // [identity]
    v["identity"] = identity::identity_json(w);
    // [looks] The figure cache: meshes alive, people outdoors and their distinct looks.
    let st = &ctx.stats;
    v["render"]["cut_meshes"] = json!(st.cut_meshes);
    v["render"]["people"] = json!({ "meshes": st.people_meshes, "mesh_cap": crate::render::PEOPLE_MESH_CAP, "visible": st.people_visible, "distinct_looks_visible": st.people_looks_visible, "built_total": st.people_built, "borrowed": st.people_borrowed, "materials": 1 });
    // The full alerts scan on every poll. Not cached: nothing cheap says
    // the world is unchanged (`step_sim` and other per-frame systems take
    // `Sim` mutably every frame, paused or not, so Bevy's change ticks
    // move each frame; the clock does not move when the player edits a
    // paused city), and a cache per sim hour would answer stale alerts.
    // The scan itself spells out ids only for the examples it keeps.
    v["alerts_top"] = report::alerts_json(&report::alerts(w), 5);
    // [districts] [tourism]
    let ts = &w.tourism;
    v["districts"] = json!(w.districts.list.len());
    v["tourism"] = json!({ "tourists": ts.count(), "attractiveness": round(ts.attractiveness), "arrivals_today": ts.stats.arrivals_today, "turned_away_today": ts.stats.turned_away_today, "spend_today": round(ts.stats.spend_today()), "tourist_tax_yesterday": round(ts.stats.tax_yesterday), "hotels": w.hotels().len() });
    v["review_hint"] = json!("for a whole-city review call report (then map, history, list)");
    v
}
