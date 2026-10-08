//! Load a save and list the parcels near a point with why each cannot be
//! zoned (the island-lots flag). Not part of the game.
//! `cargo run --example island_probe -- <save.ron> <x> <y> [radius]`
use glam::Vec2;
use nd_sim::zoning::LandUse;
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let w = World::from_ron(&text).expect("load");
    let c = Vec2::new(args[2].parse().unwrap(), args[3].parse().unwrap());
    let r: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(150.0);
    println!("half_extent {} ground {:?} parcels {}", w.config.half_extent, w.ground, w.parcels.len());
    println!("parcel_half {} ground_half {}", w.parcel_half(), w.ground_half());
    let far = w.roads.alive_segments().filter(|(_, s)| !s.is_rail() && !s.fronts_nothing()).map(|(id, _)| {
        let (a, b) = w.roads.segment_points(id);
        let m = [a, b].iter().map(|p| p.x.abs().max(p.y.abs())).filter(|c| *c < nd_sim::landscape::LAND_HALF).fold(0.0f32, f32::max);
        (m, id.0, a, b)
    }).max_by(|x, y| x.0.total_cmp(&y.0));
    println!("farthest fronting segment {far:?}");
    for half in [1100.0, w.ground_half()] {
        let t = std::time::Instant::now();
        let g = nd_sim::landscape::Ground::build(&w.landscape, &w.roads, &w.road_z, &w.road_inner, &w.lots, half);
        println!("Ground::build half {half}: {:?} in {:.0} ms", g, t.elapsed().as_secs_f64() * 1000.0);
    }
    let near = w.parcels.in_circle(c, r);
    println!("{} parcels within {r} m of {c}", near.len());
    for i in near {
        let p = &w.parcels.parcels[i];
        let mut pts = p.polygon.clone();
        pts.push(p.center);
        println!(
            "  parcel {i} at ({:.0},{:.0}) area {:.0} slope {:.2} rough {} lot {:?}: res {:?} com {:?}",
            p.center.x,
            p.center.y,
            p.area,
            w.landscape.slope_over(&pts),
            p.rough,
            p.lot,
            w.parcel_refusal(i, Some(LandUse::Residential)),
            w.parcel_refusal(i, Some(LandUse::Commercial)),
        );
    }
    let mut segs = 0;
    for (id, s) in w.roads.alive_segments() {
        let (a, b) = w.roads.segment_points(id);
        if a.distance(c) < r || b.distance(c) < r {
            segs += 1;
            println!("  segment {} {:?} street {} fronts_nothing {} ({:.0},{:.0})-({:.0},{:.0}) h {:.1}..{:.1}", id.0, (), s.is_street(), s.fronts_nothing(), a.x, a.y, b.x, b.y, w.landscape.height(a), w.landscape.height(b));
        }
    }
    println!("{segs} segments near");
}
