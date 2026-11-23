//! [r14-cliffs] Why streets and lots stand far below the natural hill.
//! Not part of the game.
//!
//! `cargo run --profile fast -p nd_sim --example cliff_probe -- <mode> <save.ron> [args]`
//! - `trace <save> x y`: the road node nearest the point, its height against
//!   the land, and the chain of neighbours that held it down.
//! - `stats <save>`: deepest cuts and fills under the ways, and how many
//!   lots float over or sink under their ground.
use nd_sim::roads::RoadKind;
use nd_sim::{Vec2, World};

fn grade(w: &World, sid: nd_sim::roads::SegmentId) -> f32 {
    let s = w.roads.segment(sid);
    let structure = s.bridge || s.viaduct || s.tunnel || s.ring || s.is_motorway() || w.roads.node(s.a).level != 0.0 || w.roads.node(s.b).level != 0.0;
    match s.kind {
        RoadKind::Rail => 0.03,
        RoadKind::Path => 0.6,
        RoadKind::Street if structure || nd_sim::roads::is_highway_spec(s.kind, s.lanes_fwd, s.lanes_bwd) => 0.07,
        RoadKind::Street if s.lanes_fwd + s.lanes_bwd >= 4 || (s.lanes_fwd >= 2 && s.lanes_bwd >= 2) => 0.11,
        RoadKind::Street => 0.25,
    }
}

fn land(w: &World, p: Vec2) -> f32 {
    w.landscape.height(p).max(nd_sim::landscape::BANK_HEIGHT)
}

fn trace(w: &World, p: Vec2) {
    let (mut best, mut bd) = (0usize, f32::MAX);
    for (k, n) in w.roads.nodes.iter().enumerate() {
        if n.segments.iter().any(|&s| w.roads.segment(s).alive) && n.pos.distance(p) < bd {
            bd = n.pos.distance(p);
            best = k;
        }
    }
    let mut k = best;
    let mut seen = std::collections::HashSet::new();
    for step in 0..400 {
        let n = &w.roads.nodes[k];
        let z = w.road_z[k];
        let lz = land(w, n.pos);
        print!("{step:3} node {k} at ({:.0},{:.0}) z {z:.1} land {lz:.1} cut {:.1} level {:.1}", n.pos.x, n.pos.y, lz - z, n.level);
        if !seen.insert(k) || n.level != 0.0 || (z - lz).abs() < 0.05 {
            println!("  <- root");
            break;
        }
        // The neighbour that binds: z_k == z_j + grade * len.
        let mut bind = None;
        for &sid in &n.segments {
            let s = w.roads.segment(sid);
            if !s.alive {
                continue;
            }
            let j = if s.a.0 as usize == k { s.b.0 as usize } else { s.a.0 as usize };
            let len = w.roads.nodes[j].pos.distance(n.pos);
            let g = grade(w, sid);
            let slack = (z - w.road_z[j]).abs() - g * len;
            if slack.abs() < 0.05 && ((w.road_z[j] < z) == (z < lz)) {
                bind = Some((j, sid, g, len, s.kind, s.lanes_fwd, s.lanes_bwd, s.is_motorway(), s.ring, s.bridge));
                if w.road_z[j] < z {
                    break;
                }
            }
        }
        match bind {
            Some((j, sid, g, len, kind, lf, lb, mw, ring, br)) => {
                println!("  <- seg {} {kind:?} {lf}+{lb} motorway {mw} ring {ring} bridge {br} grade {g} len {len:.0}", sid.0);
                k = j;
            }
            None => {
                println!("  <- no binding neighbour");
                break;
            }
        }
    }
}

fn stats(w: &World) {
    // Deepest cut/fill at road nodes (at grade only).
    let mut cuts: Vec<(f32, usize)> = Vec::new();
    let mut town_cuts: Vec<f32> = Vec::new();
    for (k, n) in w.roads.nodes.iter().enumerate() {
        if n.level != 0.0 || !n.segments.iter().any(|&s| { let s = w.roads.segment(s); s.alive && !s.bridge && !s.tunnel && !s.is_rail() }) {
            continue;
        }
        cuts.push((land(w, n.pos) - w.road_z[k], k));
        let town = n.segments.iter().all(|&s| { let s = w.roads.segment(s); !s.alive || (!s.is_motorway() && !s.is_rail() && !s.bridge && !s.tunnel && !s.ramp && !nd_sim::roads::is_highway_spec(s.kind, s.lanes_fwd, s.lanes_bwd)) });
        if town {
            town_cuts.push(land(w, n.pos) - w.road_z[k]);
        }
    }
    let town_max = town_cuts.iter().fold(0.0f32, |m, c| m.max(c.abs()));
    println!("town-street nodes: {}  |cut/fill| >6.05 m: {}  max {town_max:.1}", town_cuts.len(), town_cuts.iter().filter(|c| c.abs() > 6.05).count());
    cuts.sort_by(|a, b| b.0.total_cmp(&a.0));
    let over = |d: f32| cuts.iter().filter(|c| c.0 > d).count();
    let under = |d: f32| cuts.iter().filter(|c| c.0 < -d).count();
    println!("road nodes at grade: {}  cut>5 {} >10 {} >20 {} >40 {}  fill>5 {} >10 {} >20 {}", cuts.len(), over(5.0), over(10.0), over(20.0), over(40.0), under(5.0), under(10.0), under(20.0));
    for &(c, k) in cuts.iter().take(8) {
        let p = w.roads.nodes[k].pos;
        println!("  cut {c:.1} at node {k} ({:.0},{:.0})", p.x, p.y);
    }
    for &(c, k) in cuts.iter().rev().take(4) {
        let p = w.roads.nodes[k].pos;
        println!("  fill {:.1} at node {k} ({:.0},{:.0})", -c, p.x, p.y);
    }
    // Lots: pad against the natural ground and the built ground under it.
    let (mut floats, mut buried, mut deep_cut, mut n) = (0, 0, 0, 0);
    let mut worst: Vec<(f32, String)> = Vec::new();
    for (id, l) in w.lots.iter() {
        let Some(&pad) = w.ground.pads.get(&id) else { continue };
        if l.polygon.len() < 3 {
            continue;
        }
        n += 1;
        let step = w.ground.steps.get(&id).copied();
        // Samples round the outline: how far the pad stands over / under the built ground.
        let m = l.polygon.len();
        let (mut hang, mut sink, mut nat_cut) = (0.0f32, 0.0f32, 0.0f32);
        for k in 0..m {
            for f in [0.0, 0.5] {
                let q = l.polygon[k].lerp(l.polygon[(k + 1) % m], f);
                // just outside the lot edge
                let q2 = q + (q - l.center).normalize_or_zero() * 2.0;
                let pz = step.map_or(pad, |s| s.pad_at(q, pad));
                let g = w.ground.height(&w.landscape, q2);
                // An edge held by a retaining wall is meant to stand over
                // (or under) the ground beyond it.
                let walled = w.ground.retaining.iter().any(|r| nd_sim::geom::project_on_segment(q, r.a, r.b).1.distance(q) < 1.5);
                if !walled {
                    hang = hang.max(pz - g);
                    sink = sink.max(g - pz);
                }
                nat_cut = nat_cut.max(land(w, q) - pz);
            }
        }
        if hang > 6.0 {
            floats += 1;
        }
        if sink > 6.0 {
            buried += 1;
        }
        if nat_cut > 20.0 {
            deep_cut += 1;
        }
        let built = l.building.is_some();
        let key = if std::env::var("CLIFF_BY").as_deref() == Ok("hang") { hang.max(sink) } else { nat_cut };
        worst.push((key, format!("lot {:?} at ({:.0},{:.0}) pad {pad:.1} hang {hang:.1} sink {sink:.1} natcut {nat_cut:.1} landlocked {} built {built} step {}", id, l.center.x, l.center.y, l.landlocked, step.is_some())));
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("lots with pads: {n}  unwalled edge over ground >6 m: {floats}  ground over unwalled edge >6 m: {buried}  pad >20 m under natural: {deep_cut}");
    for (_, s) in worst.iter().take(std::env::var("CLIFF_N").ok().and_then(|v| v.parse().ok()).unwrap_or(6)) {
        println!("  {s}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let w = World::read_save(&args[2]).expect("load");
    match args[1].as_str() {
        "trace" => {
            let mut i = 3;
            while i + 1 < args.len() {
                println!("== ({}, {})", args[i], args[i + 1]);
                trace(&w, Vec2::new(args[i].parse().unwrap(), args[i + 1].parse().unwrap()));
                i += 2;
            }
        }
        "stats" => stats(&w),
        "day" => {
            // A day of play from the save: people, buildings, treasury.
            let mut w = w;
            let mut minds = nd_minds::RuleBasedMind::default();
            println!("start: pop {} bld {} treasury {:.0}", w.citizens.len(), w.buildings.len(), w.economy.treasury);
            for h in 0..24 {
                for _ in 0..60 {
                    w.tick(&mut minds);
                }
                if h % 6 == 5 {
                    println!("hour {:>2}: pop {} bld {} treasury {:.0} cars {}", h + 1, w.citizens.len(), w.buildings.len(), w.economy.treasury, w.traffic.cars.len());
                }
            }
        }
        "lot" => {
            let p = Vec2::new(args[3].parse().unwrap(), args[4].parse().unwrap());
            let Some((id, l)) = w.lots.iter().find(|(_, l)| l.contains(p)) else { println!("no lot"); return };
            let pad = w.ground.pads.get(&id).copied().unwrap_or(f32::NAN);
            println!("lot {id:?} pad {pad:.1} landlocked {} use {:?} front seg {} z {:.1}", l.landlocked, l.land_use, l.front_segment.0, w.road_height(l.front_segment, l.front_t));
            let m = l.polygon.len();
            let (lo, hi) = l.polygon.iter().map(|q| land(&w, *q)).fold((f32::MAX, f32::MIN), |(a, b), z| (a.min(z), b.max(z)));
            println!("  natural {lo:.1}..{hi:.1} terrace {} step {:?} ground {:?} building {:?} city {:?}", nd_sim::landscape::walled_terrace(&w.landscape, l, pad), w.ground.steps.get(&id), w.landscape.lot_ground(&l.polygon, l.center), l.building, l.building.and_then(|b| w.buildings.get(b)).map(|b| b.lot == id));
            println!("  lot_pads now {:?} center natural {:.1}", nd_sim::landscape::lot_pads(&w.roads, &w.road_z, &w.road_inner, l, &w.landscape), land(&w, l.center));
            for k in 0..m {
                let (a, b) = (l.polygon[k], l.polygon[(k + 1) % m]);
                let q = a.lerp(b, 0.5);
                let out = q + (q - l.center).normalize_or_zero() * 3.0;
                let other = w.lots.iter().find(|(o, ol)| *o != id && ol.contains(out)).map(|(o, _)| (o, w.ground.pads.get(&o).copied().unwrap_or(f32::NAN)));
                let road = w.roads.nearest_where(out, |_| true);
                println!("  edge {k} ({:.0},{:.0})-({:.0},{:.0}) natural {:.1} ground out {:.1} other {:?} near road {:?}", a.x, a.y, b.x, b.y, land(&w, q), w.ground.height(&w.landscape, out), other, road.map(|r| (r.0 .0, (r.1 * 10.0).round() / 10.0, (w.road_height(r.0, 0.5) * 10.0).round() / 10.0)));
            }
        }
        "rings" => {
            for r in w.roads.rings.iter() {
                let lo = (0..32).map(|k| { let a = k as f32 / 32.0 * std::f32::consts::TAU; land(&w, r.center + Vec2::new(a.cos(), a.sin()) * r.radius) }).fold((f32::MAX, f32::MIN), |(l, h), z| (l.min(z), h.max(z)));
                println!("ring at ({:.0},{:.0}) radius {:.0} lanes {} level {:.1} land round it {:.1}..{:.1}", r.center.x, r.center.y, r.radius, r.lanes, r.level, lo.0, lo.1);
            }
        }
        _ => eprintln!("modes: trace, stats"),
    }
}
