//! [r11-box-stock] The boxes each depot holds against its room, day by
//! day, from a save: stored (full, empty by owner, going out), its room
//! (what its stacks hold: [r16-tidy] the sim's own, from the yard layout
//! `nd_procgen::install_sim_hooks` installs; `DRAWN=1` compares it with
//! the room saved with the save), how many
//! of them its stacks draw, the boxes kept for it at the yards, and the
//! day's flows and trade. Not part of the game.
//!
//! cargo run --profile fast -p nd_procgen --example depot_stock_probe -- save.ron [days=4] [out.ron]
use nd_procgen::{BoxSlot, BuildingRules, Extras, Palette, BAY_TIER};
use nd_sim::containers::Owner;
use nd_sim::mind::IdleMind;
use nd_sim::World;
use std::collections::HashMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&a[1]).expect("load");
    // [r16-tidy] The yard layout's room, as the game takes it; compared
    // with the room saved (drawn by the renderer before round 16).
    nd_procgen::install_sim_hooks();
    if std::env::var("DRAWN").is_ok() {
        let saved = w.containers.layout_room.clone();
        let t = std::time::Instant::now();
        w.containers.layout_keys.clear();
        w.refresh_yard_rooms();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let (mut same, mut diff) = (0, 0);
        for (b, k, _) in w.box_depots() {
            let (old, new) = (saved.get(&b).copied(), w.containers.layout_room.get(&b).copied());
            if old == new { same += 1 } else { diff += 1 }
            println!("   depot {:?} {:?} {}: saved room {:?}, layout room {:?}", b, k, w.buildings.get(b).map(|x| x.name.as_str()).unwrap_or(""), old, new);
        }
        println!("layout rooms: {same} same as saved, {diff} differ ({} depots laid out in {ms:.1} ms)", same + diff);
    }
    let days: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    println!("loaded: day {} tick {} boxes {} people {}", w.clock.day(), w.clock.tick, w.containers.boxes.len(), w.citizens.len());
    let mut slots_of: HashMap<nd_sim::BuildingId, Vec<BoxSlot>> = HashMap::new();
    draw(&mut w, &mut slots_of);
    if std::env::var("CANDIDATES").is_ok() {
        candidates(&w);
    }
    report(&w, &slots_of);
    let mut minds = IdleMind;
    let (mut imp, mut exp, mut landed, mut deals) = (0.0f32, 0.0f32, 0u32, 0u32);
    let mut prev = w.containers.stats.clone();
    let opened0 = w.pressure.opened_total.get("container_storage").copied().unwrap_or(0);
    for _ in 0..days {
        for m in 0..1440 {
            let mk = &w.economy.market;
            let (i, e, d, l) = (mk.imports_today, mk.exports_today, mk.deals_today, w.ports.stats.freight_landed_today);
            w.tick(&mut minds);
            let mk = &w.economy.market;
            if mk.imports_today < i || mk.exports_today < e || mk.deals_today < d {
                (imp, exp, deals, landed) = (i, e, d, l);
            }
            // The renderer draws new depots (a yard opened) as they come.
            if m % 60 == 0 {
                draw(&mut w, &mut slots_of);
            }
        }
        let s = &w.containers.stats;
        println!("== day {} trade: imports {imp:.0} exports {exp:.0} deals {deals} freight landed {landed}; people {}", w.clock.day(), w.citizens.len());
        println!(
            "   flows: arrived rail/sea/road +{}/+{}/+{}, delivered +{}, sent out +{}, sold out +{}, exported +{}, scrapped +{}, to stacks +{}; to storage yards +{}, called back +{}, cleared by road +{}, overflow yday {}, storage fees {:.0}",
            s.arrived_total[0] - prev.arrived_total[0],
            s.arrived_total[1] - prev.arrived_total[1],
            s.arrived_total[2] - prev.arrived_total[2],
            s.delivered_total - prev.delivered_total,
            s.sent_out_total - prev.sent_out_total,
            s.sold_out_of_town_total - prev.sold_out_of_town_total,
            s.exported_total - prev.exported_total,
            s.scrapped_total - prev.scrapped_total,
            s.sold_to_stacks_total - prev.sold_to_stacks_total,
            s.to_storage_total - prev.to_storage_total,
            s.called_back_total - prev.called_back_total,
            s.cleared_by_road_total - prev.cleared_by_road_total,
            s.overflow_yday,
            s.storage_fees_total - prev.storage_fees_total,
        );
        let p = w.pressure.readings.iter().find(|r| r.kind == nd_sim::pressure::PressureKind::ContainerStorage);
        println!("   storage-yard pressure: {}; yards opened by it {}", p.map(|r| format!("wanted {:.1} have {} gap {:.2} held {:?}", r.wanted, r.have, r.gap, r.held)).unwrap_or_default(), w.pressure.opened_total.get("container_storage").copied().unwrap_or(0) - opened0);
        let k = nd_sim::pressure::PressureKind::ContainerStorage;
        let fits = w.lot_need_lots(k);
        let suits = fits.iter().filter(|f| f.1 == nd_sim::world::LotFit::Suits).count();
        println!(
            "   pressure state: held {} days, cooldown {}, yards being built {}, vacant industrial lots in reach {} ({} suit), hired lorries on the road {}",
            w.pressure.held_days.get(k.index()).copied().unwrap_or(0),
            w.pressure.cooldown.get(k.index()).copied().unwrap_or(0),
            w.pressure_supply().building[nd_sim::BusinessKind::ContainerYard.index()],
            fits.len(),
            suits,
            w.containers.haulers.len()
        );
        prev = s.clone();
        report(&w, &slots_of);
    }
    if let Some(out) = a.get(3) {
        let f = std::io::BufWriter::new(std::fs::File::create(out).expect("save file"));
        w.write_ron_gz(f).expect("save");
        println!("saved {out}");
    }
}

/// Each depot's stacks as its style draws them (for the report; the sim
/// takes their room itself).
fn draw(w: &mut World, slots_of: &mut HashMap<nd_sim::BuildingId, Vec<BoxSlot>>) {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    // (Every depot afresh: one may have changed trade.)
    slots_of.clear();
    for (b, _, _) in w.box_depots() {
        let Some(bld) = w.buildings.get(b) else { continue };
        let Some(lot) = w.footprint(b) else { continue };
        let extras = Extras { ..nd_procgen::style_extras(w, b, &[]) };
        let g = nd_procgen::generate_building(&lot, bld.land_use, bld.floors, bld.tier, bld.condition, bld.seed, &rules, &pal, &extras);
        let slots: Vec<BoxSlot> = g.box_slots.iter().copied().filter(|s| s.tier != BAY_TIER).collect();
        slots_of.insert(b, slots);
    }
}

fn report(w: &World, slots_of: &HashMap<nd_sim::BuildingId, Vec<BoxSlot>>) {
    let c = w.box_counts();
    println!("   boxes {} (stored {} full {}, lorries {}, ships {}, trains {}, lifting {}, sites {}, homes {}, scrap {})", c.total, c.stored, c.stored_full, c.on_trucks, c.on_ships, c.on_trains, c.lifting, c.on_sites, c.in_homes, c.at_scrap);
    let (mut over, mut undrawn) = (0, 0);
    let day = w.clock.day() as i64;
    for (b, kind, _) in w.box_depots() {
        let room = w.box_room(b, kind);
        let name = w.buildings.get(b).map(|x| x.name.clone()).unwrap_or_default();
        let (mut stored, mut full, mut line, mut yard, mut fwd, mut other, mut going, mut kept) = (0usize, 0, 0, 0, 0, 0, 0, 0);
        for r in w.containers.boxes.values().filter(|r| r.at.stack() == Some(b)) {
            stored += 1;
            if r.goods.is_some() {
                full += 1;
            } else {
                match r.owner {
                    Owner::Line => line += 1,
                    Owner::Yard(_) => yard += 1,
                    Owner::Forwarder(_) => fwd += 1,
                    _ => other += 1,
                }
            }
            if r.bound_out {
                going += 1;
            }
            if r.kept_for.is_some() {
                kept += 1;
            }
        }
        let faces = w.containers.faces_at(b, day);
        let drawn = slots_of.get(&b).map_or(0, |slots| nd_procgen::buildings::parts::stand_boxes(&faces.iter().map(|f| (f.forty, w.containers.boxes.get(f.id).and_then(|r| r.slot))).collect::<Vec<_>>(), slots).len());
        over += stored.saturating_sub(room);
        undrawn += faces.len() - drawn;
        println!(
            "   {:<30} {:<14} room {:>3} (ground {:>3}) stored {:>5} drawn {:>4} (full {:>3}, line {:>4}, yard-owned {:>3}, fwd {:>2}, other {:>2}; going out {:>4}, kept for terminals {:>3}) kept at yards {:>4}{}",
            name,
            kind.label(),
            room,
            w.box_capacity(b, kind),
            stored,
            drawn,
            full,
            line,
            yard,
            fwd,
            other,
            going,
            kept,
            w.kept_away(b),
            if stored > room { "  OVER" } else { "" }
        );
    }
    println!("   total over room: {over}, stored but not drawn: {undrawn}");
}

/// Industrial works within the storage reach of a terminal that are shut or
/// losing money: their plot, floors, area, and the stacks they would draw
/// as a container yard (for the storage pressure's conversions).
fn candidates(w: &World) {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let anchors: Vec<glam::Vec2> = w.box_depots().into_iter().filter(|d| !matches!(d.1, nd_sim::world::DepotKind::Yard | nd_sim::world::DepotKind::Scrap)).map(|d| d.2).collect();
    for (id, b) in w.buildings.iter() {
        let Some(z) = b.business.as_ref() else { continue };
        if b.land_use != nd_sim::LandUse::Industrial || !(z.closed || z.broke_days > 0 || z.losing_days(5) >= 3) {
            continue;
        }
        let Some(lot) = w.lots.get(b.lot) else { continue };
        let d = anchors.iter().map(|a| a.distance(lot.center)).fold(f32::INFINITY, f32::min);
        if d > nd_sim::containers::STORAGE_REACH {
            continue;
        }
        let Some(fp) = w.footprint(id) else { continue };
        let extras = Extras { business: Some(nd_sim::BusinessKind::ContainerYard), real_boxes: true, ..nd_procgen::style_extras(w, id, &[]) };
        let g = nd_procgen::generate_building(&fp, b.land_use, b.floors, b.tier, b.condition, b.seed, &rules, &pal, &extras);
        let slots: Vec<BoxSlot> = g.box_slots.iter().copied().filter(|s| s.tier != BAY_TIER).collect();
        println!("   candidate {:?} {} ({:?}, closed {}, broke {}, losing {}): form {:?}, floors {}, area {:.0}, {:.0} m from a terminal, room as a yard {}", id, b.name, z.kind, z.closed, z.broke_days, z.losing_days(5), b.plot.form, b.floors, lot.area(), d, nd_procgen::buildings::parts::stack_room(&slots));
    }
}
