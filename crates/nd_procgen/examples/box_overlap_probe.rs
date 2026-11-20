//! [r10-port-boxes] Stored boxes drawn inside one another, from a save:
//! every depot's slots as its style hands them over, its boxes laid in them
//! as the renderer lays them, and every pair whose volumes overlap.
//! Not part of the game.
//!
//! cargo run --profile fast -p nd_procgen --example box_overlap_probe -- save.ron [days=0]
use glam::{Vec2, Vec3};
use nd_procgen::{BoxSlot, BuildingRules, Extras, Palette, BAY_TIER, CONTAINER_20, CONTAINER_40, CONTAINER_H, CONTAINER_W};
use nd_sim::mind::IdleMind;
use nd_sim::World;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&a[1]).expect("load");
    let days: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let rules = BuildingRules::default();
    let pal = Palette::default();
    println!("day {} boxes {}", w.clock.day(), w.containers.boxes.len());
    let mut slots_of = std::collections::HashMap::new();
    for (b, kind, _) in w.box_depots() {
        let Some(bld) = w.buildings.get(b) else { continue };
        let Some(lot) = w.footprint(b) else { continue };
        let extras = Extras { ..nd_procgen::style_extras(&w, b, &[]) };
        let g = nd_procgen::generate_building(&lot, bld.land_use, bld.floors, bld.tier, bld.condition, bld.seed, &rules, &pal, &extras);
        let slots: Vec<BoxSlot> = g.box_slots.iter().copied().filter(|s| s.tier != BAY_TIER).collect();
        slots_of.insert(b, (kind, slots));
    }
    let mut minds = IdleMind;
    let old = std::env::var("OLD").is_ok();
    // Each minute, every depot's drawn boxes: how many jumped to another
    // spot while they stayed, and how many were thrown away and drawn anew
    // (their numbers gone for a moment) though they stood still.
    let mut prev: std::collections::HashMap<nd_sim::BuildingId, (Vec<nd_sim::containers::BoxId>, std::collections::HashMap<nd_sim::containers::BoxId, Vec3>)> = Default::default();
    for d in 0..=days {
        let (mut jumps, mut blinks) = (0usize, 0usize);
        if d > 0 {
            for _ in 0..1440 {
                w.tick(&mut minds);
                let day = w.clock.day() as i64;
                for (b, (_, slots)) in &slots_of {
                    let faces = w.containers.faces_at(*b, day);
                    let ids: Vec<_> = faces.iter().map(|f| f.id).collect();
                    let now: std::collections::HashMap<_, _> = laid(&w, &faces, slots, old).into_iter().map(|(k, at, _)| (faces[k].id, at)).collect();
                    if let Some((pids, p)) = prev.get(b) {
                        let changed = *pids != ids;
                        for (id, at) in &now {
                            if let Some(q) = p.get(id) {
                                if q.distance(*at) > 0.01 {
                                    jumps += 1;
                                } else if changed && old {
                                    // The old renderer drew the whole depot anew.
                                    blinks += 1;
                                }
                            }
                        }
                    }
                    prev.insert(*b, (ids, now));
                }
            }
            println!("day {}: drawn boxes that jumped to another spot {jumps}, drawn anew in place {blinks}", w.clock.day());
        }
        report(&w, &slots_of);
    }
}

fn laid(w: &World, faces: &[nd_sim::containers::BoxFace], slots: &[BoxSlot], old: bool) -> Vec<(usize, Vec3, f32)> {
    if old {
        nd_procgen::buildings::parts::lay_out_boxes(&faces.iter().map(|f| f.forty).collect::<Vec<_>>(), slots)
    } else {
        nd_procgen::buildings::parts::stand_boxes(&faces.iter().map(|f| (f.forty, w.containers.boxes.get(f.id).and_then(|r| r.slot))).collect::<Vec<_>>(), slots)
    }
}

fn report(w: &World, slots_of: &std::collections::HashMap<nd_sim::BuildingId, (nd_sim::world::DepotKind, Vec<BoxSlot>)>) {
    let day = w.clock.day() as i64;
    let (mut pairs, mut drawn, mut stored, mut slot_pairs) = (0, 0, 0, 0);
    let mut all: Vec<(nd_sim::BuildingId, Vec3, f32, f32)> = Vec::new();
    let mut ids: Vec<_> = slots_of.keys().copied().collect();
    ids.sort();
    for b in ids {
        let (kind, slots) = &slots_of[&b];
        let faces = w.containers.faces_at(b, day);
        stored += faces.len();
        // The slots themselves, as forty-footers, overlapping one another.
        let sp = overlaps(&slots.iter().map(|s| (s.at, s.yaw, if s.forty { CONTAINER_40 } else { CONTAINER_20 })).collect::<Vec<_>>());
        slot_pairs += sp.len();
        // `old`: the layout before [r10-port-boxes] (in the order the boxes
        // came, the next slot free); else by the places the sim gave them.
        let laid = laid(w, &faces, slots, std::env::var("OLD").is_ok());
        drawn += laid.len();
        let vols: Vec<(Vec3, f32, f32)> = laid.iter().map(|&(k, at, yaw)| (at, yaw, if faces[k].forty { CONTAINER_40 } else { CONTAINER_20 })).collect();
        all.extend(vols.iter().map(|v| (b, v.0, v.1, v.2)));
        let p = overlaps(&vols);
        pairs += p.len();
        let name = w.buildings.get(b).map(|x| x.name.clone()).unwrap_or_default();
        if !p.is_empty() || !sp.is_empty() || faces.len() > laid.len() {
            println!("  {:?} {} {:?}: slots {} (overlapping slot pairs {}), stored {}, drawn {}, overlapping pairs {}", b, name, kind, slots.len(), sp.len(), faces.len(), laid.len(), p.len());
            for &(i, j) in p.iter().take(4) {
                let (fi, fj) = (&faces[laid[i].0], &faces[laid[j].0]);
                println!("     {} {} at ({:.1},{:.1},{:.1}) / {} {} at ({:.1},{:.1},{:.1})", String::from_utf8_lossy(&fi.number), if fi.forty { 40 } else { 20 }, vols[i].0.x, vols[i].0.y, vols[i].0.z, String::from_utf8_lossy(&fj.number), if fj.forty { 40 } else { 20 }, vols[j].0.x, vols[j].0.y, vols[j].0.z);
            }
        }
    }
    // Boxes of two depots in one another (lots that overlap).
    let mut cross = 0;
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            if all[i].0 != all[j].0 && all[i].1.distance(all[j].1) < 14.0 && (all[i].1.y - all[j].1.y).abs() < CONTAINER_H - 0.05 && obb_overlap((all[i].1, all[i].2, all[i].3), (all[j].1, all[j].2, all[j].3)) {
                if cross < 4 {
                    println!("  cross-depot: {:?} / {:?} at ({:.1},{:.1})", all[i].0, all[j].0, all[i].1.x, all[i].1.z);
                }
                cross += 1;
            }
        }
    }
    println!("day {day}: cross-depot overlapping pairs {cross}");
    println!("day {day}: stored {stored}, drawn {drawn}, overlapping drawn pairs {pairs}, overlapping slot pairs {slot_pairs}, boxes sharing a place in the sim {}", w.containers.slot_clashes());
}

/// Pairs of boxes (floor centre, yaw, length) whose volumes overlap by
/// more than 5 cm every way.
fn overlaps(v: &[(Vec3, f32, f32)]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for i in 0..v.len() {
        for j in i + 1..v.len() {
            if (v[i].0.y - v[j].0.y).abs() > CONTAINER_H - 0.05 {
                continue;
            }
            if obb_overlap(v[i], v[j]) {
                out.push((i, j));
            }
        }
    }
    out
}

fn obb_overlap(a: (Vec3, f32, f32), b: (Vec3, f32, f32)) -> bool {
    let axes = |yaw: f32| (Vec2::new(yaw.cos(), yaw.sin()), Vec2::new(-yaw.sin(), yaw.cos()));
    let (a0, a1) = axes(a.1);
    let (b0, b1) = axes(b.1);
    let (ca, cb) = (Vec2::new(a.0.x, a.0.z), Vec2::new(b.0.x, b.0.z));
    let (ha, hb) = (Vec2::new(a.2 * 0.5 - 0.05, CONTAINER_W * 0.5 - 0.05), Vec2::new(b.2 * 0.5 - 0.05, CONTAINER_W * 0.5 - 0.05));
    for ax in [a0, a1, b0, b1] {
        let ra = ha.x * a0.dot(ax).abs() + ha.y * a1.dot(ax).abs();
        let rb = hb.x * b0.dot(ax).abs() + hb.y * b1.dot(ax).abs();
        if (cb - ca).dot(ax).abs() >= ra + rb {
            return false;
        }
    }
    true
}
