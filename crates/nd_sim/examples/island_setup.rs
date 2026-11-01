//! [ferry-islands] Lay out a ferry island on a copy of a city, for trying the
//! islands in the game: the nearest empty island (of the landscape's) to
//! the city's streets gets a ferry terminal on its shore, a pair of streets
//! and homes and shops zoned along them; the mainland shore opposite gets a
//! terminal joined to the nearest street. No bridge, no path.
//!
//! `island_setup <in.ron> <out.ron> [large|small] [days]`. Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::ports::PortKind;
use nd_sim::zoning::LandUse;
use nd_sim::{Vec2, World};

fn inside(poly: &[Vec2], p: Vec2) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

fn area(poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n).map(|i| poly[i].perp_dot(poly[(i + 1) % n])).sum::<f32>().abs() * 0.5
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&args[1]).expect("load");
    let out = &args[2];
    let kind = if args.get(3).is_some_and(|s| s == "small") { PortKind::FerryTerminal } else { PortKind::FerryTerminalLarge };
    let days: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    w.refresh_connectivity();
    w.economy.treasury = w.economy.treasury.max(400_000.0);
    let half = w.landscape.land_half() - 150.0;
    let islands: Vec<Vec<Vec2>> = w.landscape.sea.as_ref().map(|s| s.islands.clone()).unwrap_or_default();
    // Streets on the main walk network.
    let main_pts: Vec<Vec2> = w
        .connectivity_segments()
        .into_iter()
        .filter(|(id, l)| l.walk == Some(true) && w.roads.segment(*id).is_street())
        .map(|(id, _)| {
            let (a, b) = w.roads.segment_points(id);
            (a + b) * 0.5
        })
        .collect();
    let mut best: Option<(f32, usize, Vec2, Vec2)> = None;
    for (k, poly) in islands.iter().enumerate() {
        let c = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
        let a = area(poly);
        let roads = w.roads.alive_segments().filter(|(id, _)| inside(poly, w.roads.segment_points(*id).0)).count();
        println!("island {k}: centre ({:.0},{:.0}) area {:.0} m2, {roads} road pieces", c.x, c.y, a);
        if a < 60_000.0 || roads > 0 || c.x.abs().max(c.y.abs()) > half {
            continue;
        }
        let Some(q) = main_pts.iter().copied().min_by(|x, y| x.distance(c).total_cmp(&y.distance(c))) else { continue };
        let v = poly.iter().copied().min_by(|x, y| x.distance(q).total_cmp(&y.distance(q))).unwrap();
        let d = v.distance(q);
        if d < 1500.0 && best.is_none_or(|b| d < b.0) {
            best = Some((d, k, q, v));
        }
    }
    let Some((d, k, q, v)) = best else { panic!("no empty island near the streets") };
    let poly = &islands[k];
    let c = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
    println!("chose island {k}: {d:.0} m across from the street at ({:.0},{:.0})", q.x, q.y);
    // The mainland shore: from the street towards the island to the water.
    let dir = (v - q).normalize();
    let mut m = q;
    while !w.landscape.is_water(m + dir * 30.0) && m.distance(q) < d {
        m += dir * 5.0;
    }
    let town = (0..12).find_map(|i| w.place_port(kind, m - dir * (10.0 + 6.0 * i as f32) + Vec2::new(-dir.y, dir.x) * (i as f32 * 9.0 - 40.0)).ok()).expect("a terminal on the mainland shore");
    let [_, s1] = w.ports.port_of(town).unwrap().road_line();
    // Straight on to the street on the main network that was chosen.
    w.add_road(&[s1, q], 1, 1);
    // The island shore facing the town, a little inland.
    let isle = (0..16).find_map(|i| {
        let t = 18.0 + 6.0 * (i / 4) as f32;
        let side = Vec2::new(-dir.y, dir.x) * ((i % 4) as f32 * 20.0 - 30.0);
        w.place_port(kind, v + (c - v).normalize() * t + side).ok()
    }).expect("a terminal on the island shore");
    let [_, i1] = w.ports.port_of(isle).unwrap().road_line();
    // Two streets: inland from the terminal, and across at its end.
    let inland = (c - i1).normalize();
    let reach = i1.distance(c).max(120.0) + 80.0;
    let mut end = i1;
    while end.distance(i1) < reach && !w.landscape.is_water(end + inland * 40.0) {
        end += inland * 10.0;
    }
    w.add_road(&[i1, end], 1, 1);
    let across = Vec2::new(-inland.y, inland.x);
    let mid = i1.lerp(end, 0.6);
    let mut a = mid;
    while a.distance(mid) < 260.0 && !w.landscape.is_water(a + across * 40.0) {
        a += across * 10.0;
    }
    let mut b = mid;
    while b.distance(mid) < 260.0 && !w.landscape.is_water(b - across * 40.0) {
        b -= across * 10.0;
    }
    w.add_road(&[a, b], 1, 1);
    // Homes along both, shops near the terminal.
    let ids: Vec<usize> = w.parcels.in_circle(c, 600.0).into_iter().filter(|&i| inside(poly, w.parcels.parcels[i].center) && w.parcels.parcels[i].street_frontage).collect();
    let shops: Vec<usize> = ids.iter().copied().filter(|&i| w.parcels.parcels[i].center.distance(i1) < 70.0 && w.parcel_refusal(i, Some(LandUse::Commercial)).is_none()).collect();
    let homes: Vec<usize> = ids.iter().copied().filter(|i| !shops.contains(i) && w.parcel_refusal(*i, Some(LandUse::Residential)).is_none()).collect();
    w.paint_parcels(&homes, Some(LandUse::Residential), None);
    w.paint_parcels(&shops, Some(LandUse::Commercial), None);
    w.implement();
    w.refresh_connectivity();
    println!("laid: {} home parcels, {} shop parcels; terminals {} / {}", homes.len(), shops.len(), w.buildings[town].name, w.buildings[isle].name);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    for _ in 0..days * 1440 {
        w.tick(&mut minds);
    }
    for i in w.islands() {
        println!("island '{}': {} buildings, {} live, {} work; sail {:.0} min; car ferry {}", i.name, i.buildings, i.residents, i.workers, i.sail_minutes, i.car_ferry);
    }
    std::fs::write(out, w.to_ron().unwrap()).unwrap();
    println!("saved {out} (day {})", w.clock.day());
}
