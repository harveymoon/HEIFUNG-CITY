//! [r4-flags-sim] Print road pieces and their chains: `seg_dump <save> <seg>...`.
use nd_sim::World;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = World::read_save(&a[1]).expect("load");
    for s in &a[2..] {
        let mut sid = nd_sim::roads::SegmentId(s.parse().unwrap());
        // Follow the chain from this piece a few steps both ways.
        for _ in 0..1 {
            let g = w.roads.segment(sid);
            let (pa, pb) = (w.roads.node(g.a).pos, w.roads.node(g.b).pos);
            println!("seg {:?} {:?} fwd {} bwd {} car_only {:?} ramp {} a {:?} ({:.0},{:.0}) -> b {:?} ({:.0},{:.0})", sid, g.kind, g.lanes_fwd, g.lanes_bwd, g.car_only, g.ramp, g.a, pa.x, pa.y, g.b, pb.x, pb.y);
            for n in [g.a, g.b] {
                for &o in &w.roads.node(n).segments {
                    if o == sid { continue }
                    let og = w.roads.segment(o);
                    println!("   at {:?}: seg {:?} {:?} fwd {} bwd {} a {:?} b {:?}", n, o, og.kind, og.lanes_fwd, og.lanes_bwd, og.a, og.b);
                }
            }
            sid = nd_sim::roads::SegmentId(sid.0);
        }
    }
}
