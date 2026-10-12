//! [lines] Where a stop inserted into a bus line belongs along the loop
//! (not tacked on the end). The edits themselves are `World::insert_bus_stop`
//! and `World::share_bus_stop` (`world/buses.rs`).

use super::*;
use crate::geom::distance_to_polyline;

/// A stop this close (metres) to the road a leg already drives belongs in
/// that leg, however the straight-line detours compare.
pub const ON_ROUTE: f32 = 15.0;

/// Where a stop at `p` goes in a loop of `stops`, as the index it takes
/// (`0..=n`; `n` appends, which on a loop is the leg from the last stop
/// round to the first). The leg whose road passes within [`ON_ROUTE`] of
/// `p` takes it (the nearest if several do); otherwise the leg it lengthens
/// least, the closing leg winning a tie so a line drawn stop after stop
/// along a street grows at its end. `legs` are the lane polylines, one per
/// leg (stop `i` to `i + 1`); missing or stale, the legs are taken as
/// straight lines.
pub fn insert_index(stops: &[Vec2], legs: &[Vec<Vec2>], p: Vec2) -> usize {
    let n = stops.len();
    if n < 2 {
        return n;
    }
    let mut on_route: Option<(f32, f32, usize)> = None;
    let mut best: Option<(f32, usize)> = None;
    for i in 0..n {
        let (a, b) = (stops[i], stops[(i + 1) % n]);
        let detour = a.distance(p) + p.distance(b) - a.distance(b);
        let straight = [a, b];
        let poly: &[Vec2] = match legs.get(i) {
            Some(l) if legs.len() == n && l.len() >= 2 => l,
            _ => &straight,
        };
        let d = distance_to_polyline(p, poly);
        if d <= ON_ROUTE && on_route.is_none_or(|(bd, bdet, _)| d < bd - 0.5 || (d < bd + 0.5 && detour < bdet)) {
            on_route = Some((d, detour, i));
        }
        // The closing leg wins ties (half a metre of slack).
        let score = if i + 1 == n { detour - 0.5 } else { detour };
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, i));
        }
    }
    let leg = on_route.map(|(_, _, i)| i).or(best.map(|(_, i)| i)).unwrap_or(n - 1);
    leg + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn a_stop_goes_into_the_leg_it_lies_on_not_the_end() {
        // A square loop, clockwise: (0,0) (100,0) (100,100) (0,100).
        let stops = [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)];
        // On the first leg's street: between stops 1 and 2.
        assert_eq!(insert_index(&stops, &[], v(50.0, 2.0)), 1);
        // On the third leg: between 3 and 4.
        assert_eq!(insert_index(&stops, &[], v(40.0, 98.0)), 3);
        // On the closing leg (4 back to 1): appended.
        assert_eq!(insert_index(&stops, &[], v(1.0, 50.0)), 4);
    }

    #[test]
    fn off_the_route_the_cheapest_detour_wins() {
        let stops = [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)];
        // Out beyond the right side: between stops 2 and 3.
        assert_eq!(insert_index(&stops, &[], v(160.0, 50.0)), 2);
    }

    #[test]
    fn a_line_drawn_along_a_street_grows_at_its_end() {
        // Collinear stops; the next one further along appends.
        let stops = [v(0.0, 0.0), v(100.0, 0.0), v(200.0, 0.0)];
        assert_eq!(insert_index(&stops, &[], v(300.0, 0.0)), 3);
        // One or no stops: appended.
        assert_eq!(insert_index(&[v(0.0, 0.0)], &[], v(5.0, 5.0)), 1);
        assert_eq!(insert_index(&[], &[], v(5.0, 5.0)), 0);
    }

    #[test]
    fn the_road_a_leg_drives_decides_over_straight_lines() {
        // Two stops; the leg 1 -> 2 drives a long way round by y = 80, the
        // way back is straight. A stop on that road goes into leg 1 -> 2.
        let stops = [v(0.0, 0.0), v(100.0, 0.0)];
        let legs = vec![vec![v(0.0, 0.0), v(0.0, 80.0), v(100.0, 80.0), v(100.0, 0.0)], vec![v(100.0, 0.0), v(0.0, 0.0)]];
        assert_eq!(insert_index(&stops, &legs, v(50.0, 82.0)), 1);
        assert_eq!(insert_index(&stops, &legs, v(50.0, 3.0)), 2);
    }

    /// The bus test's block: two east-west streets joined at both ends.
    fn block() -> (World, RouteId) {
        let mut w = World::new(WorldConfig::test_town());
        w.add_road(&[v(-400.0, 0.0), v(400.0, 0.0)], 1, 1);
        w.add_road(&[v(-400.0, 140.0), v(400.0, 140.0)], 1, 1);
        w.add_road(&[v(-300.0, 0.0), v(-300.0, 140.0)], 1, 1);
        w.add_road(&[v(300.0, 0.0), v(300.0, 140.0)], 1, 1);
        let route = w.add_bus_route();
        for p in [v(-200.0, 4.0), v(200.0, 4.0), v(200.0, 136.0), v(-200.0, 136.0)] {
            w.add_bus_stop(route, p).expect("stop snaps to the street");
        }
        (w, route)
    }

    #[test]
    fn inserting_puts_the_stop_between_and_shifts_what_comes_after() {
        let (mut w, route) = block();
        w.transit.routes[route].stops[2].stats.on_today = 7;
        let at = w.insert_bus_stop(route, None, v(0.0, 4.0)).expect("inserted");
        assert_eq!(at, 1, "between the first two stops on the south street");
        let r = &w.transit.routes[route];
        assert_eq!(r.stops.len(), 5);
        assert_eq!(r.stops[3].stats.on_today, 7, "the figures follow their stop");
        assert!(r.stops[1].pos.distance(v(0.0, 4.0)) < 5.0);
        // Explicitly at a slot.
        let at = w.insert_bus_stop(route, Some(5), v(-296.0, 70.0)).expect("inserted");
        assert_eq!(at, 5);
        assert!(w.insert_bus_stop(route, None, v(0.0, 4.0)).is_err(), "no second stop at the same kerb");
    }

    #[test]
    fn sharing_a_stop_puts_the_line_at_the_same_kerb() {
        let (mut w, a) = block();
        let b = w.add_bus_route();
        w.add_bus_stop(b, v(296.0, 70.0)).unwrap();
        let at = w.share_bus_stop(b, (a, 1), None).expect("shared");
        let (pa, pb) = (w.transit.routes[a].stops[1], w.transit.routes[b].stops[at]);
        assert_eq!(pa.pos, pb.pos);
        assert_eq!(pa.segment, pb.segment);
        assert!(w.share_bus_stop(b, (a, 1), None).is_err(), "only once");
    }

    /// [w1] A two-way line: out along the south street and back along its
    /// far kerb, the way back kept in step with every edit.
    #[test]
    fn a_two_way_line_runs_back_along_the_far_kerb_and_follows_edits() {
        let mut w = World::new(WorldConfig::test_town());
        w.add_road(&[v(-400.0, 0.0), v(400.0, 0.0)], 1, 1);
        w.add_road(&[v(-400.0, 140.0), v(400.0, 140.0)], 1, 1);
        w.add_road(&[v(-300.0, 0.0), v(-300.0, 140.0)], 1, 1);
        w.add_road(&[v(300.0, 0.0), v(300.0, 140.0)], 1, 1);
        let route = w.add_bus_route();
        for p in [v(-200.0, 4.0), v(0.0, 4.0), v(200.0, 4.0)] {
            w.add_bus_stop(route, p).unwrap();
        }
        w.set_bus_route_two_way(route, true).unwrap();
        let r = &w.transit.routes[route];
        assert!(r.two_way);
        assert_eq!(r.stops.len(), 4, "A B C and B on the far kerb");
        assert_eq!(r.outbound_len(), 3);
        let (b, back) = (r.stops[1], r.stops[3]);
        assert!(back.back && back.segment == b.segment);
        assert!(back.pos.distance(v(0.0, -b.pos.y)) < 0.6, "opposite kerb: {:?} vs {:?}", back.pos, b.pos);
        assert!(r.runnable(), "every leg has a road: {:?}", r.broken);
        // The way back's figures stay with their stop through an edit.
        w.transit.routes[route].stops[3].stats.on_today = 9;
        let at = w.insert_bus_stop(route, None, v(100.0, 4.0)).unwrap();
        assert_eq!(at, 2, "into the way out, between B and C");
        let r = &w.transit.routes[route];
        let backs: Vec<Vec2> = r.stops[r.outbound_len()..].iter().map(|s| s.pos).collect();
        assert_eq!(backs.len(), 2);
        assert!(backs[0].x > 90.0 && backs[1].x.abs() < 1.0, "back past the new stop, then B: {backs:?}");
        assert_eq!(r.stops[5].stats.on_today, 9);
        // Removing a stop on the way back takes its twin out with it.
        w.remove_bus_stop(route, 4);
        let r = &w.transit.routes[route];
        assert_eq!(r.outbound_len(), 3);
        assert!(r.stops.iter().all(|s| (s.pos.x - 100.0).abs() > 20.0), "the new stop went, both sides");
        assert_eq!(r.stops.len(), 4);
        // Off again: a loop of the stops out.
        w.set_bus_route_two_way(route, false).unwrap();
        assert_eq!(w.transit.routes[route].stops.len(), 3);
        assert!(w.transit.routes[route].stops.iter().all(|s| !s.back));
    }

    #[test]
    fn buses_keep_their_stops_through_an_insert() {
        let (mut w, route) = block();
        // A bus heading for stop 2 that left stop 1.
        let (a, b) = (w.transit.routes[route].stops[1], w.transit.routes[route].stops[2]);
        let (lanes, s0, s1) = w.traffic.net.find_route(a.segment, a.pos, b.segment, b.pos).expect("a road between");
        let id = w.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
        w.traffic.cars[id].bus = Some(Bus { route, next_stop: 2, passengers: Vec::new(), to_depot: false, depot: BuildingId::default() });
        w.transit.departures.insert(id, (1, 0));
        let at = w.insert_bus_stop(route, Some(1), v(0.0, 4.0)).unwrap();
        assert_eq!(at, 1);
        assert_eq!(w.traffic.cars[id].bus.as_ref().unwrap().next_stop, 3, "still bound for the same stop");
        assert_eq!(w.transit.departures[&id].0, 2);
    }
}
