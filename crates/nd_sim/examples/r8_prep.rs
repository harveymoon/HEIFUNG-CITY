//! [r8] Prepare a city for the container photos: load a save, open a
//! freight forwarder on the free lot nearest the shipping port (as the
//! pressure would), run `hours`, and save. Not part of the game.
//!
//! `r8_prep <save.ron> <out.ron> [hours=30]`
use nd_sim::mind::IdleMind;
use nd_sim::{BusinessKind, LandUse, World};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hours: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(30);
    let mut w = World::read_save(&args[1]).expect("load");
    let port = w.box_depots().into_iter().find(|d| d.1 == nd_sim::world::DepotKind::Port).map(|d| d.2).unwrap_or_default();
    let mut lots: Vec<(f32, nd_sim::LotId)> = w
        .lots
        .iter()
        .filter(|(_, l)| l.building.is_none() && l.usable() && l.car_access() && matches!(l.land_use, LandUse::Industrial | LandUse::Commercial) && l.area() >= 400.0)
        .map(|(id, l)| (l.center.distance(port), id))
        .collect();
    lots.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut fw = None;
    for (_, lot) in lots.into_iter().take(20) {
        if let Ok(b) = w.open_business(lot, BusinessKind::FreightForwarder, true) {
            fw = Some(b);
            break;
        }
    }
    println!("forwarder {fw:?} {:?}", fw.map(|id| (w.buildings[id].name.clone(), w.building_lot(id).map(|l| l.center))));
    let mut minds = IdleMind;
    // This city sells next to nothing out of town (Harvey's flag 5 asked
    // "maybe we dont have any exports?"): for the photos, a few firms near
    // the forwarder have a surplus to ship.
    if let Some(b) = fw {
        let at = w.building_lot(b).map(|l| l.center).unwrap_or_default();
        let mut firms: Vec<(f32, nd_sim::BuildingId, nd_sim::Resource)> = w
            .buildings
            .iter()
            .filter_map(|(id, x)| {
                let z = x.business.as_ref()?;
                let r = z.kind.recipe().outputs.iter().map(|o| o.0).find(|r| !r.is_retail())?;
                (x.operating() && id != b).then(|| (w.building_lot(id).map_or(f32::MAX, |l| l.center.distance(at)), id, r))
            })
            .collect();
        firms.sort_by(|a, c| a.0.total_cmp(&c.0));
        let loads: Vec<(nd_sim::BuildingId, nd_sim::Resource, f32, f32)> = firms.iter().take(4).map(|f| (f.1, f.2, 14.0, 700.0)).collect();
        println!("firms {:?}", loads.iter().map(|l| (w.buildings[l.0].name.clone(), l.1)).collect::<Vec<_>>());
        w.forwarders_take_exports(&loads);
    }
    let stop_at_firm = std::env::var("STOP_AT_FIRM").is_ok();
    'outer: for h in 0..hours {
        for _ in 0..60 {
            w.tick(&mut minds);
            if stop_at_firm {
                let packing = w.containers.lorries.iter().find(|(c, j)| j.round.is_some() && w.traffic.cars.get(**c).is_some_and(|x| x.hold > 30.0 && !x.bay) && w.lorry_status(**c).is_some_and(|s| s.contains("collecting")));
                if let Some((c, _)) = packing {
                    println!("packing: car {c:?} at {:?} {:?}", w.traffic.cars[*c].pos, w.lorry_status(*c));
                    break 'outer;
                }
            }
        }
        for _ in 0..60 {
            w.tick(&mut minds);
        }
        if let Some(b) = fw {
            if let Some((loads, units, out, f)) = w.forwarder_card(b) {
                println!("h{h}: waiting {loads} ({units:.0}), out {out}, boxes out {}, loads {}, firms {}, fees {:.0}, rounds {}", f.boxes_out, f.loads_total, f.firms.len(), f.fees_total, f.rounds);
            }
        }
    }
    std::fs::write(&args[2], w.to_ron().expect("ron")).expect("write");
    println!("saved {}", args[2]);
}
