//! [ferry-routes] The city's water network as lines: every ferry route
//! (between two terminals in town, or from a terminal out to a town off
//! the map) and every shipping lane (a port's way in from elsewhere), with
//! the water path each sails, which ships are on it, how often it runs,
//! when the next boat leaves and how many people it carried today.
//!
//! Ferries are not timetabled: a terminal sends its next boat a headway
//! after the last and takes turns between its destinations
//! (`World::dispatch_ships`). The headway and next departure here replay
//! that rotation forward, so they are the plan if nothing is held up (a
//! boat still at the pier, two already out).
//!
//! Working out the paths runs the water router, so callers keep a
//! `WaterNet` and rebuild it only when `water_net_signature` changes.

use super::*;
use crate::ports::{self, PortKind, Ship, ShipId, ShipKind};
use std::hash::{Hash, Hasher};

/// The far end of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineEnd {
    /// Another terminal in town.
    Terminal(BuildingId),
    /// A town off the map (index into the terminal's `Port::remote`).
    Town(u8),
    /// Off the map for a freighter (no named port).
    Elsewhere,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Ferry,
    Freight,
}

/// One route on the water.
#[derive(Clone, Debug)]
pub struct WaterLine {
    pub kind: LineKind,
    /// The dock it is drawn from (for a route between two terminals, the
    /// one placed first).
    pub from: BuildingId,
    pub to: LineEnd,
    /// The water path from `from`'s berth to the far end; a route off the
    /// map runs on out of sight.
    pub path: Vec<Vec2>,
    /// Both ends are working docks, so boats sail it.
    pub running: bool,
    /// A colour index: ferry routes in turn, in network order.
    pub color: u8,
}

/// Points of a line further out than this are out of sight; drawing stops.
pub const VISIBLE_HALF: f32 = crate::landscape::LAND_HALF + 120.0;

impl WaterLine {
    /// Does the line call at dock `b`?
    pub fn serves(&self, b: BuildingId) -> bool {
        self.from == b || self.to == LineEnd::Terminal(b)
    }

    /// The other end, seen from `b` (one of its ends).
    pub fn other_end(&self, b: BuildingId) -> LineEnd {
        if self.from == b { self.to } else { LineEnd::Terminal(self.from) }
    }

    /// Is `s` a boat on this line?
    pub fn carries(&self, s: &Ship) -> bool {
        match self.kind {
            LineKind::Freight => s.kind == ShipKind::Freighter && s.port == self.from,
            LineKind::Ferry => {
                s.kind == ShipKind::Ferry
                    && match self.to {
                        LineEnd::Terminal(t) => (s.port == self.from && s.to == Some(t)) || (s.port == t && s.to == Some(self.from)),
                        LineEnd::Town(k) => s.port == self.from && s.remote == Some(k),
                        LineEnd::Elsewhere => false,
                    }
            }
        }
    }

    /// The path as far as it is in sight: cut where it crosses
    /// `VISIBLE_HALF` on its way out.
    pub fn visible(&self) -> Vec<Vec2> {
        let inside = |p: Vec2| p.x.abs().max(p.y.abs()) <= VISIBLE_HALF;
        let mut out = Vec::with_capacity(self.path.len());
        for (i, &p) in self.path.iter().enumerate() {
            if inside(p) {
                out.push(p);
                continue;
            }
            if let Some(&a) = (i > 0).then(|| &self.path[i - 1]).filter(|a| inside(**a)) {
                // Step out to the line of sight by halving.
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                for _ in 0..16 {
                    let m = (lo + hi) * 0.5;
                    if inside(a.lerp(p, m)) { lo = m } else { hi = m }
                }
                out.push(a.lerp(p, lo));
            }
            if i > 0 {
                break;
            }
        }
        out
    }
}

/// Every line on the water.
#[derive(Clone, Debug, Default)]
pub struct WaterNet {
    pub lines: Vec<WaterLine>,
}

impl WaterNet {
    /// Lines that call at dock `b`, by index.
    pub fn at(&self, b: BuildingId) -> Vec<usize> {
        (0..self.lines.len()).filter(|&i| self.lines[i].serves(b)).collect()
    }

    /// The line a ship is sailing, if it is a ferry or freighter on one.
    pub fn of_ship(&self, s: &Ship) -> Option<usize> {
        self.lines.iter().position(|l| l.carries(s))
    }

    fn find(&self, from: BuildingId, to: LineEnd) -> Option<usize> {
        self.lines.iter().position(|l| {
            (l.from == from && l.to == to) || matches!(to, LineEnd::Terminal(t) if l.from == t && l.to == LineEnd::Terminal(from))
        })
    }
}

/// A line as seen from one of its docks, for the inspector and the API.
#[derive(Clone, Debug)]
pub struct LineInfo {
    pub line: usize,
    pub kind: LineKind,
    pub end: LineEnd,
    pub end_name: String,
    /// Where the far end is (its berth, or where the route leaves sight).
    pub end_at: Vec2,
    pub running: bool,
    /// Boats on the line now.
    pub ships: Vec<ShipId>,
    /// Minutes between boats on the line (both ends' together), on the
    /// rotation as it stands.
    pub headway: Option<u64>,
    /// Tick of the next boat out from this dock on the line.
    pub next_departure: Option<u64>,
    /// People carried on the line today (both ways), for a ferry.
    pub passengers_today: u32,
    /// Length of the water path in sight, in metres.
    pub length: f32,
}

/// Turns of the rotation replayed for a headway: enough for every
/// destination to come round a few times.
const ROTATION_TURNS: u32 = 48;

fn key_bits(b: BuildingId) -> u64 {
    slotmap::Key::data(&b).as_ffi()
}

impl World {
    /// [ferry-routes] Changes whenever a line could have: a dock built or
    /// gone, one starting or stopping working, a terminal's towns.
    pub fn water_net_signature(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for p in self.ports.ports.values() {
            if !matches!(p.kind, PortKind::FerryTerminal | PortKind::ShippingPort) {
                continue;
            }
            (p.kind as u8).hash(&mut h);
            p.building.map(key_bits).hash(&mut h);
            p.berth.x.to_bits().hash(&mut h);
            p.berth.y.to_bits().hash(&mut h);
            p.works().hash(&mut h);
            for t in &p.remote {
                t.name.hash(&mut h);
                (t.edge as u8).hash(&mut h);
            }
        }
        self.landscape.params.seed.hash(&mut h);
        h.finish()
    }

    /// [ferry-routes] Every ferry route and shipping lane, with its water
    /// path. Runs the water router once for all of them: keep the result
    /// while `water_net_signature` stands.
    pub fn water_net(&self) -> WaterNet {
        let docks: Vec<&ports::Port> = self.ports.ports.values().filter(|p| p.building.is_some_and(|b| self.buildings.contains_key(b))).collect();
        let terminals: Vec<&ports::Port> = docks.iter().copied().filter(|p| p.kind == PortKind::FerryTerminal).collect();
        let shipping: Vec<&ports::Port> = docks.iter().copied().filter(|p| p.kind == PortKind::ShippingPort).collect();
        let mut pairs: Vec<(Vec2, Option<Vec2>)> = Vec::new();
        let mut pair_ends: Vec<(usize, usize)> = Vec::new();
        for i in 0..terminals.len() {
            for j in i + 1..terminals.len() {
                pairs.push((terminals[i].berth, Some(terminals[j].berth)));
                pair_ends.push((i, j));
            }
        }
        for p in &shipping {
            pairs.push((p.berth, None));
        }
        let outs: Vec<Vec2> = terminals.iter().filter(|p| !p.remote.is_empty()).map(|p| p.berth).collect();
        let (routes, routes_out) = self.landscape.water_routes_batch(&pairs, &outs);
        let mut lines = Vec::new();
        for (k, &(i, j)) in pair_ends.iter().enumerate() {
            let Some(path) = routes[k].clone().filter(|r| r.len() >= 2) else { continue };
            let (a, b) = (terminals[i], terminals[j]);
            let (Some(ab), Some(bb)) = (a.building, b.building) else { continue };
            let to = LineEnd::Terminal(bb);
            lines.push(WaterLine { kind: LineKind::Ferry, from: ab, to, path, running: a.works() && b.works(), color: 0 });
        }
        for (k, t) in terminals.iter().filter(|p| !p.remote.is_empty()).enumerate() {
            let Some(b) = t.building else { continue };
            for (n, town) in t.remote.iter().enumerate() {
                let Some((_, path)) = routes_out[k].iter().find(|(e, _)| *e == town.edge) else { continue };
                let to = LineEnd::Town(n as u8);
                lines.push(WaterLine { kind: LineKind::Ferry, from: b, to, path: path.clone(), running: t.works(), color: 0 });
            }
        }
        for (k, p) in shipping.iter().enumerate() {
            let Some(b) = p.building else { continue };
            let Some(path) = routes[pair_ends.len() + k].clone().filter(|r| r.len() >= 2) else { continue };
            lines.push(WaterLine { kind: LineKind::Freight, from: b, to: LineEnd::Elsewhere, path, running: p.works(), color: 0 });
        }
        // Ferry routes take the colours in turn, so no two in a small
        // network share one (the order is the docks', so it is steady).
        for (k, l) in lines.iter_mut().filter(|l| l.kind == LineKind::Ferry).enumerate() {
            l.color = k as u8;
        }
        WaterNet { lines }
    }

    /// [ferry-routes] A line's far end, seen from `b`, by name.
    pub fn line_end_name(&self, b: BuildingId, end: LineEnd) -> String {
        match end {
            LineEnd::Terminal(t) => self.buildings.get(t).map_or_else(|| "a terminal".to_string(), |x| x.name.clone()),
            LineEnd::Town(k) => self.ferry_town_name(b, k),
            LineEnd::Elsewhere => "the open sea".to_string(),
        }
    }

    /// [ferry-routes] The next tick at or after `t` inside the ferries'
    /// service hours.
    fn in_service(t: u64) -> u64 {
        let m = t % 1440;
        let day = t - m;
        if m < ports::FERRY_SERVICE_START as u64 {
            day + ports::FERRY_SERVICE_START as u64
        } else if m >= ports::FERRY_SERVICE_END as u64 {
            day + 1440 + ports::FERRY_SERVICE_START as u64
        } else {
            t
        }
    }

    /// [ferry-routes] The next `turns` sailings from ferry terminal `b`, as
    /// `dispatch_ships` would pick them: (departure tick, where to).
    fn ferry_rotation(&self, net: &WaterNet, b: BuildingId, turns: u32) -> Vec<(u64, LineEnd)> {
        let Some((pid, port)) = self.ports.ports.iter().find(|(_, p)| p.building == Some(b)) else { return Vec::new() };
        if port.kind != PortKind::FerryTerminal || !port.works() {
            return Vec::new();
        }
        let others: Vec<BuildingId> = self.ports.ports.iter().filter(|(oid, o)| *oid != pid && o.kind == PortKind::FerryTerminal && o.works()).filter_map(|(_, o)| o.building).collect();
        let has = |to: LineEnd| net.find(b, to).is_some();
        let mut t = Self::in_service((port.last_sailing + ports::FERRY_HEADWAY).max(self.clock.tick));
        let mut out = Vec::new();
        for k in 0..turns {
            let turn = port.sailings as usize + k as usize;
            let remote_turn = !port.remote.is_empty() && (others.is_empty() || turn % 2 == 0);
            let mut dest = None;
            if remote_turn {
                let n = (turn / 2) % port.remote.len();
                dest = Some(LineEnd::Town(n as u8)).filter(|&e| has(e));
            }
            if dest.is_none() {
                let start = turn / 2;
                dest = (0..others.len()).map(|i| LineEnd::Terminal(others[(start + i) % others.len()])).find(|&e| has(e));
            }
            if let Some(d) = dest {
                out.push((t, d));
            }
            t = Self::in_service(t + ports::FERRY_HEADWAY);
        }
        out
    }

    /// [ferry-routes] Every line calling at dock `b`, with its boats,
    /// headway, next departure and riders today.
    pub fn line_infos(&self, net: &WaterNet, b: BuildingId) -> Vec<LineInfo> {
        let mine = self.ferry_rotation(net, b, ROTATION_TURNS);
        let port = self.ports.port_of(b);
        net.at(b)
            .into_iter()
            .map(|i| {
                let l = &net.lines[i];
                let end = l.other_end(b);
                let vis = l.visible();
                let end_at = match end {
                    LineEnd::Terminal(t) => self.ports.port_of(t).map_or(Vec2::ZERO, |p| p.berth),
                    _ => vis.last().copied().unwrap_or(Vec2::ZERO),
                };
                let ships: Vec<ShipId> = self.ports.ships.iter().filter(|(_, s)| l.carries(s)).map(|(id, _)| id).collect();
                let (headway, next_departure, passengers_today) = match l.kind {
                    LineKind::Freight => {
                        let last = port.map_or(0, |p| p.last_sailing);
                        let waiting = self.ports.inbound.iter().any(|(p, _)| *p == b);
                        let due = last + if waiting { ports::FREIGHT_HEADWAY } else { ports::FREIGHT_HEADWAY * 2 };
                        (l.running.then_some(ports::FREIGHT_HEADWAY), l.running.then(|| due.max(self.clock.tick)), 0)
                    }
                    LineKind::Ferry => {
                        let span = ROTATION_TURNS as u64 * ports::FERRY_HEADWAY;
                        // Boats a day of rotation puts on the line, from this
                        // end and (between two terminals) from the other.
                        let mut runs = mine.iter().filter(|(_, d)| *d == end).count();
                        if let LineEnd::Terminal(t) = end {
                            runs += self.ferry_rotation(net, t, ROTATION_TURNS).iter().filter(|(_, d)| *d == LineEnd::Terminal(b)).count();
                        }
                        let headway = (runs > 0).then(|| span / runs as u64);
                        let next = mine.iter().find(|(_, d)| *d == end).map(|(t, _)| *t);
                        let riders = self
                            .ports
                            .line_riders
                            .iter()
                            .filter(|r| match end {
                                LineEnd::Terminal(t) => (r.from == b && r.to == Some(t)) || (r.from == t && r.to == Some(b)),
                                LineEnd::Town(k) => r.from == b && r.town == Some(k),
                                LineEnd::Elsewhere => false,
                            })
                            .map(|r| r.riders)
                            .sum();
                        (headway, next, riders)
                    }
                };
                LineInfo { line: i, kind: l.kind, end, end_name: self.line_end_name(b, end), end_at, running: l.running, ships, headway, next_departure, passengers_today, length: crate::geom::path_length(&vis) }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(path: Vec<Vec2>) -> WaterLine {
        WaterLine { kind: LineKind::Ferry, from: BuildingId::default(), to: LineEnd::Elsewhere, path, running: true, color: 0 }
    }

    #[test]
    fn a_route_off_the_map_is_cut_where_it_leaves_sight() {
        let l = line(vec![Vec2::new(0.0, 0.0), Vec2::new(1000.0, 0.0), Vec2::new(5000.0, 0.0), Vec2::new(5000.0, 100.0)]);
        let v = l.visible();
        assert_eq!(v.len(), 3);
        assert!((v[2].x - VISIBLE_HALF).abs() < 1.0, "{v:?}");
        let inside = line(vec![Vec2::ZERO, Vec2::new(10.0, 0.0)]);
        assert_eq!(inside.visible().len(), 2);
    }

    #[test]
    fn two_terminals_on_a_river_share_a_route_and_each_runs_off_the_map() {
        use crate::landscape::{LandscapeParams, LandscapePreset};
        let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, 3), ..WorldConfig::default() });
        w.economy.treasury = 500_000.0;
        let r = &w.landscape.rivers[0];
        let p = *r.points.iter().min_by(|a, b| a.y.abs().total_cmp(&b.y.abs())).unwrap();
        let (seg, _, _) = r.nearest(p);
        let flow = r.flow(seg);
        let n = Vec2::new(-flow.y, flow.x);
        let width = r.width;
        let mut built = Vec::new();
        for (side, along) in [(1.0, 0.0), (-1.0, 120.0), (1.0, 260.0), (-1.0, -200.0)] {
            if let Ok(b) = w.place_port(PortKind::FerryTerminal, p + n * side * (width * 0.5 + 20.0) + flow * along) {
                built.push(b);
            }
            if built.len() == 2 {
                break;
            }
        }
        assert_eq!(built.len(), 2, "two terminals on the river");
        let sig = w.water_net_signature();
        let net = w.water_net();
        let between: Vec<&WaterLine> = net.lines.iter().filter(|l| l.serves(built[0]) && l.serves(built[1])).collect();
        assert_eq!(between.len(), 1, "one route between the two: {:?}", net.lines.iter().map(|l| (l.from, l.to)).collect::<Vec<_>>());
        assert!(between[0].path.len() >= 2);
        for &b in &built {
            let towns = w.ferry_towns(b).len();
            assert_eq!(net.at(b).len(), 1 + towns, "the other terminal and its towns off the map");
            let infos = w.line_infos(&net, b);
            assert_eq!(infos.len(), 1 + towns);
            assert!(infos.iter().any(|i| i.end_name == w.buildings[if b == built[0] { built[1] } else { built[0] }].name));
        }
        // Nothing changed, nothing to rebuild; a dock gone changes it.
        assert_eq!(w.water_net_signature(), sig);
        let pid = w.ports.ports.iter().find(|(_, x)| x.building == Some(built[1])).unwrap().0;
        w.ports.ports.remove(pid);
        assert_ne!(w.water_net_signature(), sig);
    }

    #[test]
    fn ships_sail_round_islands_not_over_them() {
        use crate::landscape::{Landscape, LandscapeParams, LandscapePreset, LAND_HALF};
        let mut checked = 0;
        for seed in 1..6 {
            let land = Landscape::generate(&LandscapeParams::preset(LandscapePreset::Islands, seed));
            let Some(sea) = land.sea.as_ref() else { continue };
            for island in &sea.islands {
                let c = island.iter().copied().sum::<Vec2>() / island.len() as f32;
                let r = island.iter().map(|p| p.distance(c)).fold(0.0, f32::max);
                let (a, b) = (c + Vec2::new(r + 60.0, 0.0), c - Vec2::new(r + 60.0, 0.0));
                if !land.is_water(a) || !land.is_water(b) || a.x.abs().max(a.y.abs()) > LAND_HALF || b.x.abs().max(b.y.abs()) > LAND_HALF {
                    continue;
                }
                let way = land.sea_way(a, b);
                assert_eq!((way[0], *way.last().unwrap()), (a, b));
                for w in way.windows(2) {
                    let n = (w[0].distance(w[1]) / 5.0).ceil() as usize;
                    for k in 0..=n {
                        let p = w[0].lerp(w[1], k as f32 / n as f32);
                        assert!(land.is_water(p) || p.x.abs().max(p.y.abs()) > LAND_HALF, "seed {seed}: the way from {a} to {b} crosses land at {p}");
                    }
                }
                checked += 1;
            }
        }
        assert!(checked > 0, "some island to sail round");
    }

    #[test]
    fn service_hours_wrap_to_the_next_morning() {
        let six = ports::FERRY_SERVICE_START as u64;
        assert_eq!(World::in_service(60), six);
        assert_eq!(World::in_service(12 * 60), 12 * 60);
        assert_eq!(World::in_service(23 * 60), 1440 + six);
    }
}
