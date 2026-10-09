//! Gridlock: rings of vehicles each waiting on the next, found and broken.

use super::*;

/// A ring of full lanes whose leading vehicles each wait for the next lane
/// in the ring: nobody can move until somebody takes another way.
#[derive(Clone, Debug)]
pub struct Gridlock {
    /// Lanes of a ring of full lanes (empty for a cycle of vehicles).
    pub lanes: Vec<LaneId>,
    /// Vehicles waiting on each other in a cycle (empty for a lane ring).
    pub cycle: Vec<CarId>,
    /// The junctions the ring passes through (where its heads stand).
    pub junctions: Vec<NodeId>,
    pub points: Vec<Vec2>,
    pub vehicles: usize,
}

impl Traffic {
    /// Rings of full lanes whose leading vehicles each wait for the next
    /// lane of the ring: true gridlocks, not queues at a light.
    pub fn gridlocks(&self) -> Vec<Gridlock> {
        let waits_on = |lane: usize| -> Option<usize> {
            let l = &self.net.lanes[lane];
            if l.is_connector() {
                return None;
            }
            let c = &self.cars[*l.cars.last()?];
            if c.v >= 0.5 || l.length - c.s > 6.0 {
                return None;
            }
            let conn = *c.route.get(c.idx + 1)?;
            let exit = *c.route.get(c.idx + 2)?;
            let cl = &self.net.lanes[conn as usize];
            (cl.is_connector() && self.exit_blocked(cl, exit, true)).then_some(exit as usize)
        };
        let mut state = vec![0u8; self.net.lanes.len()];
        let mut out = Vec::new();
        for start in 0..self.net.lanes.len() {
            if state[start] != 0 {
                continue;
            }
            let mut path = Vec::new();
            let mut cur = start;
            loop {
                if state[cur] == 1 {
                    let i = path.iter().position(|&x| x == cur).unwrap_or(0);
                    let lanes: Vec<LaneId> = path[i..].iter().map(|&l| l as LaneId).collect();
                    out.push(Gridlock {
                        cycle: Vec::new(),
                        junctions: lanes.iter().map(|&l| self.net.lanes[l as usize].end_node).collect(),
                        points: lanes.iter().map(|&l| self.net.lanes[l as usize].end()).collect(),
                        vehicles: lanes.iter().map(|&l| self.net.lanes[l as usize].cars.len()).sum(),
                        lanes,
                    });
                    break;
                }
                if state[cur] == 2 {
                    break;
                }
                state[cur] = 1;
                path.push(cur);
                match waits_on(cur) {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            for p in path {
                state[p] = 2;
            }
        }
        out.extend(self.wait_cycles());
        out
    }

    /// The vehicle that a car held up by a leader further down its route,
    /// or by a full exit, is waiting for.
    pub(super) fn blocker_ahead(&self, car: &Car, why: Wait) -> Option<CarId> {
        if why == Wait::ExitFull {
            let exit = *car.route.get(car.idx + 2)?;
            return self.net.lanes[exit as usize].cars.first().copied();
        }
        let mut prev = car.route[car.idx];
        for &next in car.route.iter().skip(car.idx + 1).take(4) {
            if self.net.lanes[prev as usize].is_beside(next) {
                return None;
            }
            if let Some(&first) = self.net.lanes[next as usize].cars.first() {
                return Some(first);
            }
            prev = next;
        }
        None
    }

    /// Cycles in the wait-for graph of standing vehicles: each waits for
    /// the next (a leader, a full exit's tail, a kerb car it lets out, the
    /// car covering its kerb spot) and the last for the first. Catches the
    /// knots a lane ring does not, a kerb car and the queue letting it out
    /// included.
    pub(super) fn wait_cycles(&self) -> Vec<Gridlock> {
        let standing = |id: CarId| self.cars.get(id).is_some_and(|c| c.v < 0.5 && c.stalled > WAIT_GRAPH_STALL);
        let mut state: HashMap<CarId, u8> = HashMap::new();
        let mut out = Vec::new();
        for (start, _) in self.cars.iter() {
            if state.contains_key(&start) || !standing(start) {
                continue;
            }
            let mut path = Vec::new();
            let mut cur = start;
            loop {
                match state.get(&cur) {
                    Some(1) => {
                        let i = path.iter().position(|&x| x == cur).unwrap_or(0);
                        let cycle: Vec<CarId> = path[i..].to_vec();
                        let points: Vec<Vec2> = cycle.iter().map(|&c| self.cars[c].pos).collect();
                        let junctions = cycle.iter().map(|&c| self.net.lanes[self.cars[c].route[self.cars[c].idx] as usize].end_node).collect();
                        out.push(Gridlock { lanes: Vec::new(), vehicles: cycle.len(), cycle, junctions, points });
                        break;
                    }
                    Some(_) => break,
                    None => {}
                }
                state.insert(cur, 1);
                path.push(cur);
                match self.cars[cur].blocked_by.filter(|&b| standing(b)) {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            for p in path {
                state.insert(p, 2);
            }
        }
        out
    }
}
