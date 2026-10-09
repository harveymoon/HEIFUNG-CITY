//! [driving] Stunts and blocked junctions: cars taken off the lanes for a
//! manoeuvre of their own, and put back.

use super::*;

impl Traffic {
    /// Close (or reopen) every movement through the junction at `node`.
    pub fn set_blocked(&mut self, node: NodeId, on: bool) {
        let Some(&i) = self.net.node_intersection.get(node.0 as usize) else { return };
        let Some(inter) = self.net.intersections.get(i) else { return };
        let conns: Vec<LaneId> = inter.connectors.iter().map(|&(l, _)| l).collect();
        for l in conns {
            self.net.lanes[l as usize].blocked = on;
        }
    }

    /// Take a car off the lanes into a takeover show.
    pub fn begin_stunt(&mut self, id: CarId, kind: StuntKind) {
        let Some(c) = self.cars.get(id) else { return };
        let lane = c.route[c.idx];
        self.net.lanes[lane as usize].cars.retain(|&o| o != id);
        let c = &mut self.cars[id];
        let angle = match kind {
            StuntKind::Donut { centre, .. } => {
                let d = c.pos - centre;
                d.y.atan2(d.x)
            }
            StuntKind::Ring { .. } | StuntKind::Manoeuvre { .. } => 0.0,
        };
        c.drive.stunt = Some(Stunt { kind, angle, settled: false });
        if let Some(g) = c.drive.gang.as_mut() {
            g.phase = GangPhase::Show;
        }
        c.entering = false;
        c.passing = false;
        c.lateral = 0.0;
        c.wait = Wait::Free;
    }

    /// Back onto the lanes from a stunt, on a new route; waits at the
    /// kerb like a new car until there is a gap.
    pub fn end_stunt(&mut self, id: CarId, route: Vec<LaneId>, s0: f32, s_end: f32) {
        let Some(c) = self.cars.get_mut(id) else { return };
        if route.is_empty() {
            return;
        }
        c.drive.stunt = None;
        c.drive.path.clear();
        if let Some(g) = c.drive.gang.as_mut() {
            g.phase = GangPhase::Leaving;
        }
        c.route = route;
        c.idx = 0;
        c.s = s0;
        c.s_end = s_end;
        c.age = 0.0;
        c.v = 0.0;
        c.entering = false;
        c.stalled = 0.0;
        self.pull_out(id);
    }
}

/// Rolling to a ring spot or spinning a donut. Returns the new pose.
pub(super) fn step_stunt(st: &mut Stunt, pos: Vec2, heading: f32, dt: f32) -> (Vec2, f32) {
    let drive_to = |target: Vec2, pos: Vec2, heading: f32, speed: f32| -> (Vec2, f32, bool) {
        let d = target - pos;
        let len = d.length();
        if len < 0.3 {
            return (target, heading, true);
        }
        let step = (speed * dt).min(len);
        let dir = d / len;
        let want = dir.y.atan2(dir.x);
        (pos + dir * step, lerp_angle(heading, want, (dt * 3.0).min(1.0)), false)
    };
    match st.kind {
        StuntKind::Ring { at, face } => {
            if !st.settled {
                let (p, h, done) = drive_to(at, pos, heading, 5.0);
                st.settled = done;
                return (p, h);
            }
            (at, lerp_angle(heading, face, (dt * 1.5).min(1.0)))
        }
        StuntKind::Donut { centre, radius, dir } => {
            let on = centre + Vec2::new(st.angle.cos(), st.angle.sin()) * radius;
            if !st.settled {
                let (p, h, done) = drive_to(on, pos, heading, 6.0);
                st.settled = done || p.distance(on) < 0.6;
                return (p, h);
            }
            // Round the circle with the tail flung out: the nose points
            // inside the direction of travel.
            st.angle += dir * 2.4 * dt;
            let p = centre + Vec2::new(st.angle.cos(), st.angle.sin()) * radius;
            (p, st.angle + dir * (std::f32::consts::FRAC_PI_2 + 0.85))
        }
        // Driven by `scene::step_manoeuvre` (it needs the path).
        StuntKind::Manoeuvre { .. } => (pos, heading),
    }
}
