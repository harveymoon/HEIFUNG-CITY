//! [perfect-city] Furnishing a planned city: everything it needs put down
//! on day one through the ordinary placement calls, every result checked
//! and logged (`Furnished::log`), so the player starts at population zero
//! with the planning, zoning and buildings done.

use super::perfect::{DOWNTOWN, MIDTOWN};
use super::CityPlan;
use glam::Vec2;
use nd_sim::stories::Story;
use nd_sim::enforcement::LotSize;
use nd_sim::landscape::{HIGHWAY_Y, MAP_EDGE};
use nd_sim::world::{IndustryArea, PolicyChange};
use nd_sim::{BatteryKind, BuildingId, Gesture, IndustryKind, LandUse, Lot, LotId, Placeable, PortKind, PowerPlantKind, RailFacility, ServiceKind, Tier, World};

/// The treasury the city starts with once everything is paid for: deep
/// enough that it never runs dry while it grows (Harvey's "unlimited
/// money"), while taxes stay at their standard rates so the economy
/// behaves as usual.
pub const STARTING_TREASURY: f32 = 50_000_000.0;

/// What furnishing put down, in words (one line per placement, refusals
/// included), and the counts.
#[derive(Clone, Debug, Default)]
pub struct Furnished {
    pub log: Vec<String>,
    pub placed: usize,
    pub refused: usize,
}

struct Ctx<'a> {
    w: &'a mut World,
    out: Furnished,
    downtown: Vec2,
    midtown: Vec2,
    works: Vec<Vec2>,
    farms: Vec<Vec2>,
    /// The lots people will live and shop on: what services cover.
    serve: Vec<Vec2>,
}

fn dist_to_line(p: Vec2, pts: &[Vec2]) -> f32 {
    pts.windows(2).map(|w| nd_sim::geom::project_on_segment(p, w[0], w[1]).1.distance(p)).fold(f32::INFINITY, f32::min)
}

fn nearest(p: Vec2, pts: &[Vec2]) -> f32 {
    pts.iter().map(|q| q.distance(p)).fold(f32::INFINITY, f32::min)
}

fn circle(c: Vec2, r: f32) -> Vec<Vec2> {
    (0..24).map(|k| c + Vec2::from_angle(k as f32 / 24.0 * std::f32::consts::TAU) * r).collect()
}

fn industrial(l: &Lot) -> bool {
    l.land_use == LandUse::Industrial
}

fn not_park(l: &Lot) -> bool {
    l.land_use != LandUse::Park
}

fn civic_or_shops(l: &Lot) -> bool {
    matches!(l.land_use, LandUse::Civic | LandUse::Commercial | LandUse::Residential)
}

impl Ctx<'_> {
    fn note(&mut self, s: String) {
        self.out.log.push(s);
    }

    fn placed(&mut self, what: &str, b: BuildingId) {
        self.out.placed += 1;
        let name = self.w.buildings[b].name.clone();
        let at = self.w.lots.get(self.w.buildings[b].lot).map_or(Vec2::ZERO, |l| l.center);
        self.note(format!("{what}: {name} at ({:.0}, {:.0})", at.x, at.y));
    }

    fn refused(&mut self, what: &str, why: &str) {
        self.out.refused += 1;
        self.note(format!("{what}: REFUSED ({why})"));
    }

    fn centre_of(&self, b: BuildingId) -> Vec2 {
        self.w.lots.get(self.w.buildings[b].lot).map_or(Vec2::ZERO, |l| l.center)
    }

    /// Vacant, usable lots with a street out front: (id, centre, area).
    fn open_lots(&self) -> Vec<(LotId, Vec2, f32)> {
        let mut v: Vec<(LotId, Vec2, f32)> = self
            .w
            .lots
            .iter()
            .filter(|(_, l)| l.usable() && l.car_access() && l.building.is_none() && l.park.is_none() && !self.w.is_city_ground(l))
            .map(|(id, l)| (id, l.center, l.area()))
            .collect();
        // Slot-map order is the order lots were made: deterministic, but
        // sort by position anyway so ties break the same way every time.
        v.sort_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.1.y.total_cmp(&b.1.y)));
        v
    }

    /// Put `what` on the best open lot that `ok` accepts, by `cost` (lowest
    /// first; infinite rules a lot out), trying a few before giving up.
    fn place(&mut self, what_label: &str, what: Placeable, ok: impl Fn(&Lot) -> bool, cost: impl Fn(Vec2, f32) -> f32) -> Option<BuildingId> {
        let need = what.min_area();
        let mut cands: Vec<(f32, LotId)> = self
            .open_lots()
            .into_iter()
            // [big-sites] Or a lot its neighbours join to make room.
            .filter(|&(id, _, area)| (area >= need || self.w.joined_site(what, id).is_some()) && self.w.lots.get(id).is_some_and(&ok))
            .map(|(id, c, area)| (cost(c, area), id))
            .filter(|(c, _)| c.is_finite())
            .collect();
        cands.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut last = "no lot fits".to_string();
        for &(_, lot) in cands.iter().take(12) {
            match self.w.place_on_lot(what, lot) {
                Ok(id) => {
                    self.placed(what_label, id);
                    return Some(id);
                }
                Err(e) => last = e,
            }
        }
        self.refused(what_label, &last);
        None
    }

    /// Greedy cover: stations of `kind` (the first near `first` at
    /// `first_tier`, the rest at `tier`) until `share` of the served lots
    /// lie within reach of one (straight line, a little short of the road
    /// reach the sim measures).
    #[allow(clippy::too_many_arguments)]
    fn cover(&mut self, kind: ServiceKind, first: Vec2, first_tier: Tier, tier: Tier, share: f32, max: usize, ok: fn(&Lot) -> bool) {
        let reach = |t: Tier| kind.radius(t) * 0.6;
        let label = kind.label();
        let mut stations: Vec<(Vec2, f32)> = Vec::new();
        if let Some(id) = self.place(label, Placeable::Service(kind, first_tier), ok, |c, _| c.distance(first)) {
            stations.push((self.centre_of(id), reach(first_tier)));
        }
        let n = self.serve.len().max(1) as f32;
        let covered = |p: Vec2, st: &[(Vec2, f32)]| st.iter().any(|(c, r)| c.distance(p) <= *r);
        for _ in 0..max {
            let left: Vec<Vec2> = self.serve.iter().copied().filter(|p| !covered(*p, &stations)).collect();
            if left.len() as f32 <= n * (1.0 - share) {
                break;
            }
            let r = reach(tier);
            let gain = |c: Vec2| left.iter().filter(|p| p.distance(c) <= r).count();
            match self.place(label, Placeable::Service(kind, tier), ok, |c, _| -(gain(c) as f32)) {
                Some(id) => stations.push((self.centre_of(id), r)),
                None => break,
            }
        }
        let left = self.serve.iter().filter(|p| !covered(**p, &stations)).count();
        self.note(format!("{label}: {} in all, {:.0}% of homes and shops within reach", stations.len(), 100.0 * (1.0 - left as f32 / n)));
    }

    /// Junctions of ordinary streets within `r` of `p`.
    fn street_nodes_near(&self, p: Vec2, r: f32) -> Vec<Vec2> {
        let roads = &self.w.roads;
        roads
            .nodes
            .iter()
            .filter(|n| n.pos.distance(p) <= r)
            .filter(|n| n.segments.iter().any(|&s| roads.try_segment(s).is_some_and(|s| s.alive && s.is_street() && !s.is_highway())))
            .map(|n| n.pos)
            .collect()
    }

    /// The nearest point on a two-way street (not the highway) within `max`.
    fn street_point(&self, p: Vec2, max: f32) -> Option<Vec2> {
        let roads = &self.w.roads;
        roads
            .segments_in_box(p - Vec2::splat(max), p + Vec2::splat(max))
            .filter(|&s| {
                let s = roads.segment(s);
                s.alive && s.is_street() && !s.is_highway() && s.lanes_fwd > 0 && s.lanes_bwd > 0
            })
            .map(|s| {
                let (a, b) = roads.segment_points(s);
                nd_sim::geom::project_on_segment(p, a, b).1
            })
            .filter(|q| q.distance(p) <= max)
            .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
    }

    /// A dock or station whose street stub is not on the network yet: a
    /// road from each free end to the nearest junction.
    /// [w1-roads] For a rail building refused over a street running along
    /// the track ("a road runs across the site at (x, y)"): take that
    /// street's stretch between its junctions out and build. True if one
    /// was taken out.
    fn clear_railside(&mut self, why: &str) -> bool {
        let Some(rest) = why.split("runs across the site at (").nth(1) else { return false };
        let nums: Vec<f32> = rest.split(')').next().unwrap_or("").split(',').filter_map(|t| t.trim().parse().ok()).collect();
        let [x, y] = nums[..] else { return false };
        let at = Vec2::new(x, y);
        let Some((sid, _, q)) = self.w.roads.nearest_street(at) else { return false };
        let (a, b) = self.w.roads.segment_points(sid);
        // Only a street along the track (east-west) or a dead end coming up
        // to it, never one crossing it.
        let dead_end = |p: Vec2| self.w.roads.node_near(p, 0.5).is_some_and(|n| self.w.roads.street_degree(n) == 1);
        let s = self.w.roads.segment(sid);
        let run_dead = self.w.road_run(sid).iter().any(|&r| {
            let (ra, rb) = self.w.roads.segment_points(r);
            dead_end(ra) || dead_end(rb)
        });
        if q.distance(at) > 2.0 || ((b - a).normalize_or_zero().x.abs() < 0.7 && !run_dead) || s.is_motorway() {
            return false;
        }
        let gid = self.w.master.gestures.iter().find(|(_, g)| matches!(g, Gesture::Road { .. }) && nd_sim::geom::distance_to_polyline(at, &g.road_polyline()) < 1.5).map(|(id, _)| *id);
        let Some(gid) = gid else { return false };
        let Some((s0, s1)) = self.w.road_piece_span(gid, Some(sid), at) else { return false };
        self.w.remove_road_piece(gid, s0, s1);
        self.w.implement();
        self.note(format!("railside street at ({x:.0}, {y:.0}) taken out for the station"));
        true
    }

    fn join_stub(&mut self, b: BuildingId, what: &str) {
        // [w1-roads] A stub left dangling is joined too, even when the
        // building reaches a street another way (a station whose
        // railside street gave way to it fronts the streets crossing the
        // track, and its stub was a dead piece cut off from the city).
        if self.w.road_access(b) != Some(false) && self.w.stub_ends_of(b).is_empty() {
            return;
        }
        // [w1-roads] Never back onto the stub's own other end (a road along
        // the stub joins nothing).
        let own: Vec<Vec2> = self.w.ports.port_of(b).and_then(|p| p.stub).map(|s| s.to_vec()).unwrap_or_default();
        for end in self.w.stub_ends_of(b) {
            let to = self.street_nodes_near(end, 240.0).into_iter().filter(|q| q.distance(end) > 3.0 && own.iter().all(|o| o.distance(*q) > 1.0)).min_by(|a, c| a.distance(end).total_cmp(&c.distance(end)));
            if let Some(to) = to {
                self.w.add_road(&[end, to], 1, 1);
            }
        }
        let ok = self.w.road_access(b) == Some(true);
        self.note(format!("{what}: {}", if ok { "joined to the streets" } else { "STILL OFF THE STREETS" }));
    }
}

/// Lay everything a planned city needs on top of the streets and zoning
/// (`lay_city` has implemented them).
pub fn furnish(w: &mut World, plan: &CityPlan) -> Furnished {
    // Paid for out of a deep purse; topped up to the starting treasury at
    // the end.
    w.economy.treasury = w.economy.treasury.max(STARTING_TREASURY * 2.0);
    let downtown = plan.cores.first().copied().unwrap_or(DOWNTOWN);
    let midtown = plan.cores.get(1).copied().unwrap_or(MIDTOWN);
    let works: Vec<Vec2> = plan.areas.iter().filter(|a| a.0 == "works").map(|a| a.1).collect();
    let farms: Vec<Vec2> = plan.areas.iter().filter(|a| a.0 == "farms").map(|a| a.1).collect();
    let serve: Vec<Vec2> = w.lots.values().filter(|l| l.usable() && matches!(l.land_use, LandUse::Residential | LandUse::Commercial | LandUse::Entertainment)).map(|l| l.center).collect();
    let mut cx = Ctx { w, out: Furnished::default(), downtown, midtown, works, farms, serve };
    interchanges(&mut cx, plan);
    bridges(&mut cx);
    power(&mut cx);
    rail_and_docks(&mut cx);
    services(&mut cx);
    transit(&mut cx);
    beach(&mut cx);
    districts(&mut cx);
    footbridges(&mut cx);
    let Ctx { w, out, .. } = cx;
    w.economy.treasury = STARTING_TREASURY;
    out
}

/// Diamond interchanges where the plan's streets reach the highway.
fn interchanges(cx: &mut Ctx, plan: &CityPlan) {
    for &l in &plan.links {
        let (a, b) = (l, Vec2::new(l.x, HIGHWAY_Y + 90.0));
        match cx.w.add_diamond(a, b, (2, 2)) {
            Ok(_) => {
                cx.w.implement();
                let line = match cx.w.implement_refused.clone() {
                    None => format!("diamond interchange at x {:.0}", l.x),
                    Some(why) => format!("diamond interchange at x {:.0}: REFUSED on implement ({why})", l.x),
                };
                cx.note(line);
            }
            Err(r) => {
                let ok = cx.w.link_to_highway(l);
                cx.note(format!("diamond interchange at x {:.0}: refused ({}); plain ramps instead: {ok}", l.x, r.why));
            }
        }
    }
}

/// More bridges over the main river: the works are across it, and the
/// traced streets cross it only every few hundred metres, mostly on the
/// skew. A straight 2+2 bridge joins the nearest junctions on either bank
/// wherever a crossing is more than `GAP` from the next.
fn bridges(cx: &mut Ctx) {
    const GAP: f32 = 150.0;
    let Some(river) = cx.w.landscape.rivers.iter().filter(|r| !r.stream).max_by(|a, b| a.width.total_cmp(&b.width)).cloned() else { return };
    let lo = cx.w.lots.values().map(|l| l.center).fold(Vec2::splat(f32::INFINITY), Vec2::min);
    let hi = cx.w.lots.values().map(|l| l.center).fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
    // Where streets cross it now: the middles of the pieces over the water.
    let mut crossings: Vec<f32> = cx
        .w
        .roads
        .alive_segments()
        .filter(|(_, sg)| sg.is_street() && !sg.is_highway())
        .map(|(id, _)| {
            let (a, b) = cx.w.roads.segment_points(id);
            (a + b) * 0.5
        })
        .filter(|m| cx.w.landscape.water_at(*m).is_some_and(|h| h.width >= river.width * 0.8))
        .map(|m| m.y)
        .collect();
    let mut made = 0;
    let mut y = lo.y + 120.0;
    while y < hi.y - 60.0 {
        let here = y;
        y += 40.0;
        if crossings.iter().any(|c| (c - here).abs() < GAP) {
            continue;
        }
        let Some(rp) = river.points.iter().copied().filter(|p| (p.y - here).abs() < 40.0).min_by(|a, b| (a.y - here).abs().total_cmp(&(b.y - here).abs())) else { continue };
        let half = river.width * 0.5;
        let bank = |side: f32| {
            let mut v: Vec<Vec2> = cx.street_nodes_near(rp, half + 320.0).into_iter().filter(|q| (q.x - rp.x) * side > half + 15.0 && (q.y - here).abs() < 60.0).collect();
            v.sort_by(|a, b| a.distance(rp).total_cmp(&b.distance(rp)).then(a.x.total_cmp(&b.x)));
            v.truncate(4);
            v
        };
        let (ws, es) = (bank(-1.0), bank(1.0));
        // Each end becomes a junction: none other within 25 m of it, and
        // the bridge meets every street there at 40 degrees or more.
        let roads = &cx.w.roads;
        let fits = |p: Vec2, other: Vec2| {
            let dir = (other - p).normalize_or_zero();
            roads.nodes.iter().all(|n| {
                let d = n.pos.distance(p);
                if d < 0.5 {
                    return n.segments.iter().filter_map(|&s| roads.try_segment(s)).filter(|s| s.alive).all(|s| {
                        let (a, b) = (roads.node(s.a).pos, roads.node(s.b).pos);
                        let out = if a.distance(p) < 0.5 { b - a } else { a - b };
                        out.normalize_or_zero().dot(dir) < 40f32.to_radians().cos()
                    });
                }
                let junction = n.segments.iter().filter(|&&s| roads.try_segment(s).is_some_and(|s| s.alive)).count() != 2;
                d >= 25.0 || !junction
            })
        };
        let clear = |w: Vec2, e: Vec2| {
            roads.segments_in_box(w.min(e) - Vec2::splat(2.0), w.max(e) + Vec2::splat(2.0)).all(|s| {
                let (a, b) = roads.segment_points(s);
                a.distance(w) < 1.0 || a.distance(e) < 1.0 || b.distance(w) < 1.0 || b.distance(e) < 1.0 || !segments_cross(a, b, w, e)
            })
        };
        let pair = ws.iter().flat_map(|&w| es.iter().map(move |&e| (w, e))).find(|&(w, e)| (w.y - e.y).abs() <= 70.0 && fits(w, e) && fits(e, w) && clear(w, e));
        let Some((w, e)) = pair else { continue };
        cx.w.add_road(&[w, e], 2, 2);
        if cx.w.implement_refused.is_some() {
            continue;
        }
        crossings.push((w.y + e.y) * 0.5);
        made += 1;
        cx.note(format!("bridge over the river at y {:.0}: ({:.0}, {:.0}) to ({:.0}, {:.0})", (w.y + e.y) * 0.5, w.x, w.y, e.x, e.y));
    }
    if made == 0 {
        cx.note("bridges: the river crossings were close enough already".into());
    }
}

fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let (r, s) = (b - a, d - c);
    let den = r.perp_dot(s);
    if den.abs() < 1e-6 {
        return false;
    }
    let t = (c - a).perp_dot(s) / den;
    let u = (c - a).perp_dot(r) / den;
    (0.01..0.99).contains(&t) && (0.01..0.99).contains(&u)
}

/// Try `place` at each offset from `p` until one takes.
fn place_near(cx: &mut Ctx, what: &str, offsets: &[Vec2], mut place: impl FnMut(&mut World, Vec2) -> Result<BuildingId, String>, p: Vec2) -> Option<BuildingId> {
    let mut last = String::from("nowhere to try");
    for &d in offsets {
        match place(cx.w, p + d) {
            Ok(b) => {
                cx.placed(what, b);
                cx.join_stub(b, what);
                return Some(b);
            }
            Err(e) => last = e,
        }
    }
    cx.refused(what, &last);
    None
}

/// The central station by downtown, stations either side, the freight yard
/// in the works, a ferry terminal by downtown and a shipping port by the
/// works.
fn rail_and_docks(cx: &mut Ctx) {
    let y = nd_sim::rail::TRUNK_Y;
    // Streets cross the line at grade: a train standing at a platform must
    // not stand over a crossing (its barriers would stay down), so each
    // station goes in the middle of the gap between two crossings nearest
    // where it is wanted.
    let mut xs: Vec<f32> = cx
        .w
        .roads
        .alive_segments()
        .filter(|(_, sg)| !sg.is_rail())
        .filter_map(|(id, _)| {
            let (a, b) = cx.w.roads.segment_points(id);
            ((a.y - y) * (b.y - y) <= 0.0 && (a.y - b.y).abs() > 1e-3).then(|| a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y))
        })
        .collect();
    xs.sort_by(f32::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 5.0);
    let gap_mid = |want: f32, need: f32, taken: &[f32]| -> Vec2 {
        let mut best: Option<(f32, f32)> = None;
        for w in xs.windows(2) {
            let (mid, width) = ((w[0] + w[1]) * 0.5, w[1] - w[0]);
            if width < need || taken.iter().any(|t| (t - mid).abs() < 150.0) {
                continue;
            }
            let d = (mid - want).abs();
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, mid));
            }
        }
        Vec2::new(best.map_or(want, |b| b.1), y)
    };
    // [w1-roads] Either side of the track (a station goes on the side of
    // the click), and off any street running along it: nothing is built
    // over a road.
    let along: Vec<Vec2> = [0.0f32, 15.0, -15.0, 30.0, -30.0, 60.0, -60.0].iter().flat_map(|&x| [Vec2::new(x, 0.0), Vec2::new(x, 3.0), Vec2::new(x, -3.0)]).collect();
    let mut taken: Vec<f32> = Vec::new();
    let east = cx.works.iter().map(|p| p.x).fold(cx.downtown.x + 300.0, f32::max);
    let stations = [
        (RailFacility::Central, cx.downtown.x, "central station"),
        (RailFacility::Station, cx.downtown.x - 520.0, "west station"),
        (RailFacility::Station, (cx.downtown.x + east) * 0.5, "east station"),
        (RailFacility::FreightYard, east + 60.0, "freight yard"),
    ];
    for (kind, want, what) in stations {
        let p = gap_mid(want, kind.platform_length() + 140.0, &taken);
        // [w1-roads] Nothing is built over a road: where a street runs
        // alongside the track too close for the building's depth, the next
        // gaps out are tried (nearest first).
        let mut gaps: Vec<f32> = xs.windows(2).filter(|w| w[1] - w[0] >= kind.platform_length() + 140.0).map(|w| (w[0] + w[1]) * 0.5).filter(|m| taken.iter().all(|t| (t - m).abs() >= 150.0) && (m - p.x).abs() > 1.0).collect();
        gaps.sort_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs()));
        let mut tries: Vec<Vec2> = along.clone();
        tries.extend(gaps.iter().flat_map(|g| along.iter().map(move |o| Vec2::new(g - p.x, 0.0) + *o)));
        let kinds: Vec<RailFacility> = vec![kind];
        let mut placed = None;
        let mut last = String::from("nowhere to try");
        'kinds: for k in kinds {
            for &d in &tries {
                match cx.w.place_rail_facility(k, p + d) {
                    Ok(b) => {
                        placed = Some(b);
                        break 'kinds;
                    }
                    Err(e) => last = e,
                }
            }
        }
        // [w1-roads] Nowhere clear: the stretch of the street running along
        // the track that the building would stand over gives way to it (the
        // station fronts the streets crossing the track instead), at the
        // spot first wanted.
        if placed.is_none() && kind != RailFacility::Station {
            let centres: Vec<f32> = std::iter::once(p.x).chain(gaps.iter().copied().take(4)).collect();
            'sides: for (cx_x, side) in centres.iter().flat_map(|x| [(*x, 3.0f32), (*x, -3.0)]) {
                for _ in 0..4 {
                    match cx.w.place_rail_facility(kind, Vec2::new(cx_x, p.y + side)) {
                        Ok(b) => {
                            placed = Some(b);
                            break 'sides;
                        }
                        Err(e) => {
                            last = e.clone();
                            if !cx.clear_railside(&e) {
                                break;
                            }
                        }
                    }
                }
            }
        }
        match placed {
            Some(b) => {
                cx.placed(what, b);
                cx.join_stub(b, what);
                taken.push(cx.centre_of(b).x);
            }
            None => cx.refused(what, &last),
        }
    }
    let coast: Vec<Vec2> = cx.w.landscape.sea.as_ref().map(|s| s.coast.clone()).unwrap_or_default();
    if coast.len() >= 2 {
        // A point on dry land just inland of the coast line (the sea is to
        // the south).
        let shore = |x: f32| coast.iter().copied().min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs())).unwrap_or(Vec2::new(x, -700.0)) + Vec2::new(0.0, 35.0);
        let offs: Vec<Vec2> = [0.0, 70.0, -70.0, 140.0, -140.0, 210.0, -210.0, 280.0, -280.0, 350.0, -350.0, 420.0, -420.0].iter().map(|&x| Vec2::new(x, 0.0)).collect();
        let ferry_x = cx.downtown.x + 180.0;
        // [w1-roads] Then on along the shore, both ways, to a stretch with
        // no street along it inside the terminal's yard.
        let far: Vec<Vec2> = (1..40).flat_map(|k| [Vec2::new(420.0 + 35.0 * k as f32, 0.0), Vec2::new(-420.0 - 35.0 * k as f32, 0.0)]).collect();
        let offs_f: Vec<Vec2> = offs.iter().chain(far.iter()).map(|o| shore(ferry_x + o.x) - Vec2::new(ferry_x, 0.0)).collect();
        place_near(cx, "ferry terminal", &offs_f, |w, p| w.place_port(PortKind::FerryTerminal, p), Vec2::new(ferry_x, 0.0));
        let port_x = east - 40.0;
        let offs_p: Vec<Vec2> = offs.iter().map(|o| shore(port_x + o.x) - Vec2::new(port_x, 0.0)).collect();
        place_near(cx, "shipping port", &offs_p, |w, p| w.place_port(PortKind::ShippingPort, p), Vec2::new(port_x, 0.0));
    }
}

/// A sensible mix: hydro on the river, geothermal on the hot spots (no
/// tidal race: the sea front is beaches and parks, with no street down to
/// the shore outside the docks), wind parks on the farms, solar fields in the works,
/// batteries for the evening peak, one coal plant for base load early and
/// two nuclear plants for the big city, all far downwind.
fn power(cx: &mut Ctx) {
    let rivers: Vec<Vec<Vec2>> = cx.w.landscape.rivers.iter().filter(|r| r.current >= 0.8).map(|r| r.points.clone()).collect();
    for k in 0..2 {
        let taken: Vec<Vec2> = cx.w.buildings.values().filter(|b| b.plant == Some(PowerPlantKind::Hydro)).filter_map(|b| cx.w.lots.get(b.lot).map(|l| l.center)).collect();
        let rv = rivers.clone();
        let cost = move |c: Vec2, _: f32| {
            let d = rv.iter().map(|r| dist_to_line(c, r)).fold(f32::INFINITY, f32::min);
            if d > 90.0 || nearest(c, &taken) < 250.0 { f32::INFINITY } else { d }
        };
        if cx.place(&format!("hydro {}", k + 1), Placeable::Plant(PowerPlantKind::Hydro), |l| matches!(l.land_use, LandUse::Industrial | LandUse::Park | LandUse::Civic), cost).is_none() {
            break;
        }
    }
    // Geothermal: every hot spot the city reaches; beyond the highway, a
    // road out to it from the nearest interchange's far side.
    let spots: Vec<Vec2> = cx.w.landscape.hot_spots.iter().map(|h| h.pos).collect();
    for (k, s) in spots.into_iter().enumerate() {
        let what = format!("geothermal {}", k + 1);
        if s.y > HIGHWAY_Y - 40.0 {
            let end = cx.street_nodes_near(s, 900.0).into_iter().filter(|p| p.y > HIGHWAY_Y + 40.0).min_by(|a, b| a.distance(s).total_cmp(&b.distance(s)));
            match end {
                Some(e) if s.x.abs() < MAP_EDGE - 100.0 && s.y < MAP_EDGE - 100.0 => {
                    cx.w.add_road(&[e, s + Vec2::new(0.0, 30.0)], 1, 1);
                    cx.note(format!("access road out to the hot spot at ({:.0}, {:.0})", s.x, s.y));
                }
                _ => {
                    cx.note(format!("{what}: hot spot at ({:.0}, {:.0}) out of reach", s.x, s.y));
                    continue;
                }
            }
        }
        let mut offs = vec![Vec2::ZERO];
        for r in [22.0, 40.0] {
            offs.extend((0..8).map(|a| Vec2::from_angle(a as f32 * std::f32::consts::FRAC_PI_4) * r));
        }
        let mut last = String::new();
        let mut done = false;
        for d in offs {
            match cx.w.place_building(Placeable::Plant(PowerPlantKind::Geothermal), s + d) {
                Ok(b) => {
                    cx.placed(&what, b);
                    done = true;
                    break;
                }
                Err(e) => last = e,
            }
        }
        if !done {
            cx.refused(&what, &last);
        }
    }
    // Wind parks on the farmland (upwind, open ground), big lots first.
    for k in 0..6 {
        let farms = cx.farms.clone();
        let cost = move |c: Vec2, area: f32| {
            let d = nearest(c, &farms);
            if d > 420.0 { f32::INFINITY } else { d - area * 0.05 }
        };
        if cx.place(&format!("wind park {}", k + 1), Placeable::Plant(PowerPlantKind::Wind), industrial, cost).is_none() {
            break;
        }
    }
    // Solar fields: the biggest works lots.
    for k in 0..6 {
        let works = cx.works.clone();
        let cost = move |c: Vec2, area: f32| {
            let d = nearest(c, &works);
            if d > 450.0 { f32::INFINITY } else { d - area }
        };
        if cx.place(&format!("solar field {}", k + 1), Placeable::Plant(PowerPlantKind::Solar), industrial, cost).is_none() {
            break;
        }
    }
    // Base load: one coal plant on a standard lot, as far downwind (east)
    // as the works go.
    cx.place("coal plant (base load)", Placeable::Plant(PowerPlantKind::Coal), industrial, |c, area| -c.x * 2.0 - c.y * 0.3 + (area - nd_sim::power::COAL_REFERENCE_AREA).abs() * 0.2);
    // Clean base load for the city it grows into (an industrial floor
    // draws 0.5 MW: a 25,000-person city needs some 1.5 GW): two nuclear
    // plants at the far east of the works, downwind and away from homes.
    for k in 0..2 {
        cx.place(&format!("nuclear plant {}", k + 1), Placeable::Plant(PowerPlantKind::Nuclear), industrial, |c, area| -c.x * 2.0 - area * 0.05);
    }
    // Batteries for the evening peak, in the works.
    for k in 0..6 {
        let works = cx.works.clone();
        cx.place(&format!("battery {}", k + 1), Placeable::Battery(BatteryKind::Large), industrial, move |c, _| nearest(c, &works));
    }
}

/// Police, fire, health and schools to cover every neighbourhood, a
/// university, the 911 centre, a jail and trash depots by the works, a
/// monument by the downtown square, a stadium, car parks and wardens, a
/// vertiport and a cargo hub.
fn services(cx: &mut Ctx) {
    let (dt, mt) = (cx.downtown, cx.midtown);
    cx.place("911 dispatch", Placeable::Service(ServiceKind::Dispatch, Tier::High), civic_or_shops, |c, _| c.distance(dt + Vec2::new(0.0, 120.0)));
    cx.cover(ServiceKind::Police, dt + Vec2::new(0.0, 150.0), Tier::High, Tier::Mid, 0.99, 10, civic_or_shops);
    cx.cover(ServiceKind::Fire, mt, Tier::High, Tier::Mid, 0.99, 10, civic_or_shops);
    cx.cover(ServiceKind::Health, dt + Vec2::new(150.0, 250.0), Tier::High, Tier::Mid, 0.99, 8, civic_or_shops);
    cx.cover(ServiceKind::School, mt + Vec2::new(150.0, -150.0), Tier::High, Tier::High, 0.99, 12, civic_or_shops);
    cx.place("university", Placeable::Service(ServiceKind::University, Tier::High), not_park, |c, _| c.distance(mt + Vec2::new(0.0, 160.0)));
    let works = cx.works.clone();
    let w2 = works.clone();
    cx.place("jail", Placeable::Service(ServiceKind::Jail, Tier::High), industrial, move |c, _| nearest(c, &w2));
    // Trash depots on works and civic ground, each where it reaches most of
    // what is still out of reach.
    // (Each depot's trucks serve only so many bins a day: a reach well short
    // of its radius, so a big city gets several.)
    let r = ServiceKind::TrashDepot.radius(Tier::High) * 0.55;
    let mut depots: Vec<Vec2> = Vec::new();
    for _ in 0..7 {
        let left: Vec<Vec2> = cx.serve.iter().copied().filter(|p| nearest(*p, &depots) > r).collect();
        if left.len() * 50 < cx.serve.len() {
            break;
        }
        let gain = |c: Vec2| left.iter().filter(|p| p.distance(c) <= r).count() as f32;
        match cx.place("trash depot", Placeable::Service(ServiceKind::TrashDepot, Tier::High), |l| matches!(l.land_use, LandUse::Industrial | LandUse::Civic), |c, _| -gain(c)) {
            Some(b) => depots.push(cx.centre_of(b)),
            None => break,
        }
    }
    cx.place("monument", Placeable::Service(ServiceKind::Monument, Tier::High), not_park, |c, _| c.distance(dt));
    cx.place("stadium", Placeable::Service(ServiceKind::Stadium, Tier::High), not_park, move |c, _| c.distance(mt + Vec2::new(320.0, 140.0)));
    cx.place("parking wardens", Placeable::Service(ServiceKind::ParkingEnforcement, Tier::High), civic_or_shops, |c, _| c.distance(dt + Vec2::new(0.0, 200.0)));
    cx.place("vertiport", Placeable::Service(ServiceKind::Vertiport, Tier::Mid), |l| l.land_use == LandUse::Commercial, |c, _| c.distance(dt + Vec2::new(220.0, 80.0)));
    // Car parks on the edge of the core, by the avenues in, not in it.
    for (k, off) in [Vec2::new(300.0, 200.0), Vec2::new(-300.0, 200.0), Vec2::new(0.0, 420.0)].into_iter().enumerate() {
        cx.place(&format!("car park {}", k + 1), Placeable::ParkingLot(LotSize::MultiStorey), |l| l.land_use == LandUse::Commercial, move |c, _| c.distance(dt + off));
    }
    cx.place("car park (midtown)", Placeable::ParkingLot(LotSize::MultiStorey), |l| l.land_use == LandUse::Commercial, move |c, _| c.distance(mt));
    cx.place("cargo hub", Placeable::CargoHub, industrial, move |c, _| nearest(c, &works));
}

/// Two metro lines (north-south from the central station, east-west through
/// midtown and downtown), two bus depots and bus lines along the avenues.
fn transit(cx: &mut Ctx) {
    let (dt, mt) = (cx.downtown, cx.midtown);
    let central: Option<BuildingId> = cx.w.buildings.iter().find(|(_, b)| b.rail.as_ref().is_some_and(|r| r.kind == RailFacility::Central)).map(|(id, _)| id);
    let lines: [Vec<Vec2>; 2] = [
        vec![dt + Vec2::new(0.0, 60.0), dt + Vec2::new(10.0, 270.0), dt + Vec2::new(20.0, 480.0), dt + Vec2::new(20.0, 640.0)],
        vec![mt + Vec2::new(-400.0, -20.0), mt + Vec2::new(-200.0, 0.0), mt, mt + Vec2::new(210.0, -60.0), dt + Vec2::new(10.0, 270.0), dt + Vec2::new(270.0, 160.0), dt + Vec2::new(520.0, 160.0)],
    ];
    let mut known: Vec<(Vec2, BuildingId)> = central.map(|c| (cx.centre_of(c), c)).into_iter().collect();
    for (k, pts) in lines.iter().enumerate() {
        let mut stations: Vec<BuildingId> = Vec::new();
        if k == 0 {
            stations.extend(central);
        }
        for &p in pts {
            if let Some(&(_, b)) = known.iter().find(|(q, _)| q.distance(p) < 140.0) {
                if !stations.contains(&b) {
                    stations.push(b);
                }
                continue;
            }
            let near: Vec<Vec2> = known.iter().map(|q| q.0).collect();
            if let Some(b) = cx.place("metro station", Placeable::MetroStation, not_park, move |c, _| if nearest(c, &near) < 175.0 || c.distance(p) > 160.0 { f32::INFINITY } else { c.distance(p) }) {
                known.push((cx.centre_of(b), b));
                stations.push(b);
            }
        }
        match cx.w.add_metro_line(&stations) {
            Ok(id) => {
                let _ = cx.w.set_metro_headway(id, 6);
                cx.note(format!("metro line {}: {} stations, a train every 6 minutes", k + 1, stations.len()));
            }
            Err(e) => cx.refused(&format!("metro line {}", k + 1), &e),
        }
    }
    let works = cx.works.clone();
    let depot_ok = |l: &Lot| matches!(l.land_use, LandUse::Industrial | LandUse::Civic | LandUse::Commercial);
    cx.place("bus depot (east)", Placeable::BusDepot, depot_ok, move |c, _| nearest(c, &works) + c.distance(dt) * 0.6);
    cx.place("bus depot (west)", Placeable::BusDepot, depot_ok, move |c, _| c.distance(mt + Vec2::new(-280.0, -260.0)));
    // Bus lines: stops about every 280 m along each line's course, each on
    // the nearest two-way street.
    let lo = cx.w.lots.values().map(|l| l.center).fold(Vec2::splat(f32::INFINITY), Vec2::min);
    let hi = cx.w.lots.values().map(|l| l.center).fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
    let routes: Vec<(&str, Vec<Vec2>)> = vec![
        ("Crosstown", vec![Vec2::new(lo.x + 80.0, mt.y), Vec2::new(hi.x - 80.0, mt.y)]),
        ("Seafront", vec![Vec2::new(lo.x + 80.0, dt.y - 110.0), Vec2::new(hi.x - 80.0, dt.y - 110.0)]),
        ("Uptown", vec![Vec2::new(lo.x + 80.0, mt.y + 230.0), Vec2::new(hi.x - 80.0, mt.y + 230.0)]),
        ("Westside", vec![Vec2::new(mt.x - 220.0, lo.y + 60.0), Vec2::new(mt.x - 220.0, hi.y - 60.0)]),
        ("Eastside", vec![Vec2::new(dt.x + 340.0, lo.y + 60.0), Vec2::new(dt.x + 340.0, hi.y - 60.0)]),
        ("Downtown Loop", vec![dt + Vec2::new(-200.0, -40.0), dt + Vec2::new(200.0, -40.0), dt + Vec2::new(200.0, 280.0), dt + Vec2::new(-200.0, 280.0), dt + Vec2::new(-200.0, -40.0)]),
    ];
    for (name, course) in routes {
        let mut stops: Vec<Vec2> = Vec::new();
        for leg in course.windows(2) {
            let n = (leg[0].distance(leg[1]) / 280.0).ceil().max(1.0) as usize;
            for i in 0..=n {
                let p = leg[0].lerp(leg[1], i as f32 / n as f32);
                if let Some(q) = cx.street_point(p, 90.0) {
                    if stops.iter().all(|s| s.distance(q) > 150.0) {
                        stops.push(q);
                    }
                }
            }
        }
        if stops.len() < 3 {
            cx.refused(&format!("bus line {name}"), &format!("only {} stops on streets", stops.len()));
            continue;
        }
        let route = cx.w.add_bus_route();
        let ok = stops.iter().filter(|s| cx.w.add_bus_stop(route, **s).is_ok()).count();
        cx.w.set_route_buses(route, (ok / 3).clamp(2, 6) as u8);
        if let Some(r) = cx.w.transit.routes.get_mut(route) {
            r.name = name.to_string();
        }
        cx.note(format!("bus line {name}: {ok} stops"));
    }
}

/// A public beach on the sea front either side of downtown.
fn beach(cx: &mut Ctx) {
    let dt = cx.downtown;
    let ids: Vec<usize> = (0..cx.w.parcels.parcels.len())
        .filter(|&i| {
            let p = &cx.w.parcels.parcels[i];
            p.land_use == Some(LandUse::Park) && (p.center.x - dt.x).abs() < 650.0 && cx.w.beach_refusal(i).is_none()
        })
        .collect();
    if ids.is_empty() {
        cx.note("public beach: no park runs down to the sea".into());
        return;
    }
    let (_, n) = cx.w.paint_parcels_with(&ids, Some(LandUse::Park), Some(nd_sim::shore::BEACH_SUBZONE), None);
    cx.w.implement();
    cx.note(format!("public beach: {n} parcels of sand along the sea front"));
}

/// Districts with their stories: plain mid-rise homes up to 60 m across the
/// north (no story), Quiet Acres by the farms, the Walled Quarter by the
/// railway, the Promenade on the sea front, Afterglow Mile in midtown,
/// Founders' Campus downtown, Foundry Row at the works and an Eco Village
/// over the farms.
fn districts(cx: &mut Ctx) {
    // [stories] Each district a story, with the demo's own height limit.
    let story = |s: Story, height: Option<f32>| PolicyChange { story: Some(Some(s)), height_limit: Some(height), ..Default::default() };
    let (dt, mt) = (cx.downtown, cx.midtown);
    let lo = cx.w.lots.values().map(|l| l.center).fold(Vec2::splat(f32::INFINITY), Vec2::min);
    let hi = cx.w.lots.values().map(|l| l.center).fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
    let rect = |a: Vec2, b: Vec2| vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)];
    let rail = nd_sim::rail::TRUNK_Y;
    let farm_x = cx.farms.iter().map(|f| f.x + 260.0).fold(lo.x + 250.0, f32::max);
    let works_x = cx.works.iter().map(|f| f.x - 280.0).fold(hi.x - 300.0, f32::min);
    let ds: Vec<(&str, Vec<Vec2>, PolicyChange)> = vec![
        // Painted in order: later districts take the cells they share.
        // Plain homes at every tier (no look pulling the rents up), mid-rise.
        ("Northside", rect(Vec2::new(farm_x, rail + 60.0), Vec2::new(works_x, hi.y + 60.0)), PolicyChange { height_limit: Some(Some(60.0)), ..Default::default() }),
        ("Garden Suburbs", rect(Vec2::new(farm_x, rail + 60.0), Vec2::new(farm_x + 260.0, hi.y + 60.0)), story(Story::QuietAcres, Some(24.0))),
        // Cheap dense housing for the works' and the shops' staff.
        ("Railside", rect(Vec2::new(farm_x, rail - 120.0), Vec2::new(works_x, rail + 260.0)), story(Story::WalledQuarter, None)),
        ("Sea Front", rect(Vec2::new(farm_x, lo.y - 80.0), Vec2::new(works_x, rail - 120.0)), story(Story::Promenade, Some(60.0))),
        ("Midtown", circle(mt, 240.0), story(Story::AfterglowMile, None)),
        ("Downtown", circle(dt + Vec2::new(0.0, 130.0), 300.0), story(Story::FoundersCampus, None)),
        ("Eastworks", rect(Vec2::new(works_x, lo.y - 60.0), Vec2::new(hi.x + 60.0, hi.y + 60.0)), story(Story::FoundryRow, None)),
        ("West Farms", rect(Vec2::new(lo.x - 60.0, lo.y - 60.0), Vec2::new(farm_x, hi.y + 60.0)), story(Story::EcoVillage, Some(18.0))),
    ];
    for (name, area, change) in ds {
        let r = cx.w.create_district(Some(name.to_string())).and_then(|id| {
            let cells = cx.w.paint_district_polygon(&area, id)?;
            cx.w.set_district_policy(id, change)?;
            Ok(cells)
        });
        let line = match r {
            Ok(cells) => format!("district {name}: {cells} cells"),
            Err(e) => format!("district {name}: REFUSED ({e})"),
        };
        cx.note(line);
    }
    // Logistics yards by the highway in the north-east works.
    if let Some(&dep) = cx.works.iter().max_by(|a, b| a.y.total_cmp(&b.y)) {
        if dep.y > 0.0 {
            let r = cx.w.set_industry_bias(IndustryArea::Polygon(circle(dep, 210.0)), Some(IndustryKind::Logistics));
            cx.note(format!("logistics yards by the highway: {r:?}"));
        }
    }
}

/// Footbridges over the avenues at the busiest crossings: round the
/// central station and downtown.
fn footbridges(cx: &mut Ctx) {
    let dt = cx.downtown + Vec2::new(0.0, 150.0);
    let station = cx.w.buildings.values().find(|b| b.rail.as_ref().is_some_and(|r| r.kind == RailFacility::Central)).and_then(|b| cx.w.lots.get(b.lot)).map_or(dt, |l| l.center);
    let roads = &cx.w.roads;
    let mut cands: Vec<(f32, u32)> = cx
        .w
        .walk
        .crossings
        .iter()
        .enumerate()
        .filter(|(_, c)| c.node.is_some() && !c.bridged && roads.try_segment(c.segment).is_some_and(|s| s.lanes_fwd + s.lanes_bwd >= 4 && !s.is_highway()))
        .map(|(k, c)| (c.mid.distance(station).min(c.mid.distance(dt)), k as u32))
        .filter(|(d, _)| *d < 320.0)
        .collect();
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut n = 0;
    for (_, k) in cands {
        if n >= 4 {
            break;
        }
        if cx.w.set_crossing_upgrade(k, nd_sim::junctions::CrossingUpgrade::Footbridge).is_ok() {
            n += 1;
        }
    }
    cx.note(format!("footbridges: {n} over the avenues by the station and downtown"));
}
