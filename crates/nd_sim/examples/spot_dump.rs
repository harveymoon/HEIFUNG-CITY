//! [r4-flags-sim] What is at a spot of a save: lots (use, landlocked,
//! building, floors, deck cap) and road pieces (kind, lanes, use, levels,
//! walkable). `spot_dump <save.ron> x y [r]`. Not part of the game.
use nd_sim::World;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = World::read_save(&a[1]).expect("load");
    let p = nd_sim::Vec2::new(a[2].parse().unwrap(), a[3].parse().unwrap());
    let r: f32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(60.0);
    println!("day {} {:02}:{:02}", w.clock.day(), w.clock.hour(), w.clock.minute());
    for (id, l) in w.lots.iter() {
        if l.center.distance(p) > r {
            continue;
        }
        let b = l.building.and_then(|b| w.buildings.get(b));
        println!(
            "lot {:?} ({:.0},{:.0}) area {:.0} {:?} landlocked {} front_seg {:?} deck_cap {:?} building {}",
            id,
            l.center.x,
            l.center.y,
            l.area(),
            l.land_use,
            l.landlocked,
            l.front_segment,
            w.deck_cap_of(&l.polygon),
            b.map_or("-".into(), |b| format!("{} floors {}", b.name, b.floors))
        );
    }
    for sid in w.roads.segments_in_box(p - nd_sim::Vec2::splat(r), p + nd_sim::Vec2::splat(r)) {
        let g = w.roads.segment(sid);
        let (pa, pb) = (w.roads.node(g.a).pos, w.roads.node(g.b).pos);
        println!(
            "seg {:?} {:?} fwd {} bwd {} car_only {:?} ramp {} ring {} tunnel {} motorway {} walkable {} a {:?}({:.0},{:.0} lv {:.1} n{}) b {:?}({:.0},{:.0} lv {:.1} n{}) spans_lots {}",
            sid, g.kind, g.lanes_fwd, g.lanes_bwd, g.car_only, g.ramp, g.ring, g.tunnel, g.is_motorway(), g.walkable(),
            g.a, pa.x, pa.y, w.roads.node(g.a).level, w.roads.node(g.a).segments.len(),
            g.b, pb.x, pb.y, w.roads.node(g.b).level, w.roads.node(g.b).segments.len(),
            w.roads.spans_lots(sid)
        );
    }
}
