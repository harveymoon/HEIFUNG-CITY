//! [w1] Editing a metro line stop by stop, as bus lines are: insert a
//! station (where it belongs along the line, or at a given place), remove
//! one, move one to another station, open or close the loop. Each edit
//! re-lays the line's tunnel through its new stations; only the tunnel
//! between stations that were not neighbours before is bored and paid for.
//! Trains already out finish the run they started on the old tunnel;
//! people waiting for a ride to or from a station that left the line go on
//! by other means.

use super::*;
use crate::subway::{self as metro, MetroLineId};

/// Unordered pairs of neighbouring stations (the tunnel legs) of a line
/// through `stations`, the last back to the first on a loop.
fn legs(stations: &[BuildingId], looped: bool) -> Vec<(BuildingId, BuildingId)> {
    let n = stations.len();
    let mut out: Vec<(BuildingId, BuildingId)> = stations.windows(2).map(|w| (w[0].min(w[1]), w[0].max(w[1]))).collect();
    if looped && n >= 3 {
        out.push((stations[0].min(stations[n - 1]), stations[0].max(stations[n - 1])));
    }
    out
}

/// Where a station at `p` goes in a line through `stops` (platform points):
/// the index it takes, `0..=n`. On a loop as a bus loop
/// (`bus_edit::insert_index`); on a line with two ends, into the leg it
/// lengthens least, or onto either end when that is shorter (the far end
/// winning a tie, so a line drawn outwards grows at its end).
pub fn metro_insert_index(stops: &[Vec2], looped: bool, p: Vec2) -> usize {
    let n = stops.len();
    if looped && n >= 3 {
        return super::bus_edit::insert_index(stops, &[], p);
    }
    if n < 2 {
        return n;
    }
    let mut best = (stops[n - 1].distance(p) - 0.5, n);
    let front = stops[0].distance(p);
    if front < best.0 {
        best = (front, 0);
    }
    for i in 0..n - 1 {
        let (a, b) = (stops[i], stops[i + 1]);
        let detour = a.distance(p) + p.distance(b) - a.distance(b);
        if detour < best.0 {
            best = (detour, i + 1);
        }
    }
    best.1
}

impl World {
    /// [w1] The cost of re-laying line `id` through `stations` (in order;
    /// a loop when `looped`): the tunnel between stations that are not
    /// neighbours on the line now. Checks the stations as a new line's.
    pub fn metro_relay_cost(&self, id: MetroLineId, stations: &[BuildingId], looped: bool) -> Result<f32, String> {
        let l = self.subway.lines.get(id).ok_or("no such line")?;
        let (seen, looped) = self.metro_calls(&Self::calls_of(stations, looped))?;
        let pts: Vec<Vec2> = seen.iter().filter_map(|&s| self.metro_platform(s)).collect();
        let p = metro::preview(&pts, looped, l.headway, self.config.traffic_seconds_per_tick);
        let old = legs(&l.stations, l.looped);
        let new = legs(&seen, looped);
        let bored: f32 = new.iter().zip(&p.legs_m).filter(|(leg, _)| !old.contains(leg)).map(|(_, m)| *m).sum();
        Ok(bored * metro::TUNNEL_COST_PER_M)
    }

    fn calls_of(stations: &[BuildingId], looped: bool) -> Vec<BuildingId> {
        let mut calls = stations.to_vec();
        if looped && stations.len() >= 3 {
            calls.push(stations[0]);
        }
        calls
    }

    /// [w1] Re-lay line `id` through `stations` (in order; a loop when
    /// `looped` and three or more) and pay for the new tunnel. The cost.
    pub fn relay_metro_line(&mut self, id: MetroLineId, stations: &[BuildingId], looped: bool) -> Result<f32, String> {
        let cost = self.metro_relay_cost(id, stations, looped)?;
        if cost > self.economy.treasury {
            return Err(format!("the treasury has {:.0}; the new tunnel costs {:.0}", self.economy.treasury, cost));
        }
        let (seen, looped) = self.metro_calls(&Self::calls_of(stations, looped))?;
        let pts: Vec<Vec2> = seen.iter().filter_map(|&s| self.metro_platform(s)).collect();
        let (path, stops) = metro::lay_line(&pts, looped);
        if cost > 0.0 {
            self.economy.debit(Flow::Construction, cost);
        }
        let l = &mut self.subway.lines[id];
        l.stations = seen.clone();
        l.looped = looped;
        l.path = path;
        l.stops = stops;
        self.strand_metro_waiting(|line, board, alight| line == id && !(seen.contains(&board) && seen.contains(&alight)));
        self.subway.version += 1;
        Ok(cost)
    }

    /// [w1] Put `station` into line `id`: at `at` (0 before the first,
    /// the number of stations after the last), or with no `at` where it
    /// belongs along the line ([`metro_insert_index`]). The index it took.
    pub fn insert_metro_stop(&mut self, id: MetroLineId, at: Option<usize>, station: BuildingId) -> Result<usize, String> {
        let l = self.subway.lines.get(id).ok_or("no such line")?;
        if l.stations.contains(&station) {
            return Err(format!("{} is already on {}", self.buildings.get(station).map_or("that station", |b| b.name.as_str()), l.name));
        }
        let p = self.metro_platform(station).ok_or("that is not a metro station")?;
        let pts: Vec<Vec2> = l.stations.iter().filter_map(|&s| self.metro_platform(s)).collect();
        let idx = at.unwrap_or_else(|| metro_insert_index(&pts, l.looped, p)).min(l.stations.len());
        let mut stations = l.stations.clone();
        let looped = l.looped;
        stations.insert(idx, station);
        self.relay_metro_line(id, &stations, looped)?;
        Ok(idx)
    }

    /// [w1] Take station `idx` off line `id`. A line keeps two stations at
    /// least (delete the line to close it); a loop left with two runs end
    /// to end.
    pub fn remove_metro_stop(&mut self, id: MetroLineId, idx: usize) -> Result<(), String> {
        let l = self.subway.lines.get(id).ok_or("no such line")?;
        if idx >= l.stations.len() {
            return Err("no such stop".into());
        }
        if l.stations.len() <= 2 {
            return Err("a line needs two stations: delete the line instead".into());
        }
        let mut stations = l.stations.clone();
        let looped = l.looped && stations.len() > 3;
        stations.remove(idx);
        self.relay_metro_line(id, &stations, looped).map(|_| ())
    }

    /// [w1] Call at `station` instead of stop `idx` of line `id`.
    pub fn move_metro_stop(&mut self, id: MetroLineId, idx: usize, station: BuildingId) -> Result<(), String> {
        let l = self.subway.lines.get(id).ok_or("no such line")?;
        if idx >= l.stations.len() {
            return Err("no such stop".into());
        }
        if l.stations[idx] == station {
            return Ok(());
        }
        if l.stations.contains(&station) {
            return Err(format!("{} is already on {}", self.buildings.get(station).map_or("that station", |b| b.name.as_str()), l.name));
        }
        let mut stations = l.stations.clone();
        let looped = l.looped;
        stations[idx] = station;
        self.relay_metro_line(id, &stations, looped).map(|_| ())
    }

    /// [w1] Close line `id` into a loop (three stations or more) or open
    /// it again to run end to end.
    pub fn set_metro_looped(&mut self, id: MetroLineId, looped: bool) -> Result<(), String> {
        let l = self.subway.lines.get(id).ok_or("no such line")?;
        if l.looped == looped {
            return Ok(());
        }
        if looped && l.stations.len() < 3 {
            return Err("a loop needs three stations or more".into());
        }
        let stations = l.stations.clone();
        self.relay_metro_line(id, &stations, looped).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn a_station_goes_into_the_leg_it_lies_on_or_onto_the_nearer_end() {
        let line = [v(0.0, 0.0), v(500.0, 0.0), v(1000.0, 0.0)];
        assert_eq!(metro_insert_index(&line, false, v(250.0, 20.0)), 1);
        assert_eq!(metro_insert_index(&line, false, v(750.0, -30.0)), 2);
        assert_eq!(metro_insert_index(&line, false, v(1400.0, 0.0)), 3, "beyond the far end: appended");
        assert_eq!(metro_insert_index(&line, false, v(-400.0, 0.0)), 0, "beyond the first: in front");
        assert_eq!(metro_insert_index(&[v(0.0, 0.0)], false, v(5.0, 0.0)), 1);
        // A loop inserts as a bus loop does.
        let sq = [v(0.0, 0.0), v(500.0, 0.0), v(500.0, 500.0), v(0.0, 500.0)];
        assert_eq!(metro_insert_index(&sq, true, v(-20.0, 250.0)), 4, "on the leg home");
    }

    fn town() -> World {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-1200.0, 0.0), Vec2::new(1200.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-1150.0, 4.0), Vec2::new(1150.0, 4.0), Vec2::new(1150.0, 60.0), Vec2::new(-1150.0, 60.0)], LandUse::Residential);
        w.add_zone(vec![Vec2::new(-1150.0, -4.0), Vec2::new(1150.0, -4.0), Vec2::new(1150.0, -60.0), Vec2::new(-1150.0, -60.0)], LandUse::Commercial);
        w.implement();
        w.economy.treasury = 1_000_000.0;
        w
    }

    fn station(w: &mut World, x: f32) -> BuildingId {
        let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).min_by(|a, b| a.1.center.distance(v(x, 30.0)).total_cmp(&b.1.center.distance(v(x, 30.0)))).map(|(id, _)| id).unwrap();
        w.build_metro_station(lot).expect("a station")
    }

    #[test]
    fn stops_are_inserted_removed_and_moved_and_only_new_tunnel_is_paid_for() {
        let mut w = town();
        let a = station(&mut w, -900.0);
        let c = station(&mut w, 900.0);
        let line = w.add_metro_line(&[a, c]).unwrap();
        let b = station(&mut w, 0.0);
        let d = station(&mut w, 450.0);
        // In between: the tunnel A-C is replaced by A-B and B-C.
        let before = w.economy.treasury;
        let at = w.insert_metro_stop(line, None, b).expect("inserted");
        assert_eq!(at, 1);
        assert_eq!(w.subway.lines[line].stations, vec![a, b, c]);
        let paid = before - w.economy.treasury;
        assert!(paid > 1500.0 * metro::TUNNEL_COST_PER_M && paid < 2000.0 * metro::TUNNEL_COST_PER_M, "both new legs paid: {paid}");
        assert_eq!(w.subway.lines[line].stops.len(), 3);
        assert!(w.insert_metro_stop(line, None, b).is_err(), "only once");
        // Moving C to D bores only D's new leg.
        let before = w.economy.treasury;
        w.move_metro_stop(line, 2, d).expect("moved");
        assert_eq!(w.subway.lines[line].stations, vec![a, b, d]);
        let paid = before - w.economy.treasury;
        assert!(paid > 300.0 * metro::TUNNEL_COST_PER_M && paid < 600.0 * metro::TUNNEL_COST_PER_M, "the B-D leg: {paid}");
        // Removing re-joins the neighbours; a line keeps two.
        w.remove_metro_stop(line, 1).expect("removed");
        assert_eq!(w.subway.lines[line].stations, vec![a, d]);
        assert!(w.remove_metro_stop(line, 0).is_err(), "two stations at least");
        // Close and open the loop.
        w.insert_metro_stop(line, Some(2), c).unwrap();
        w.set_metro_looped(line, true).expect("a loop");
        assert!(w.subway.lines[line].looped);
        let p = &w.subway.lines[line].path;
        assert!(p[0].distance(*p.last().unwrap()) < 1.0, "the tunnel comes home");
        w.set_metro_looped(line, false).unwrap();
        assert!(!w.subway.lines[line].looped);
        // No money: refused, nothing changed.
        w.economy.treasury = 0.0;
        assert!(w.insert_metro_stop(line, None, b).is_err());
        assert_eq!(w.subway.lines[line].stations, vec![a, d, c]);
    }
}
