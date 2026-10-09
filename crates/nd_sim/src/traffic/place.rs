//! Where each car is drawn after a step: its pose on its lane.

use super::*;

impl Traffic {
    /// Positions and headings after a substep: lane, lane-change drift,
    /// weave, long vehicles swinging wide, stunts. Records the pose for
    /// drawing (`Car::pose_at`).
    pub(super) fn place_cars(&mut self, dt: f32) {
        for car in self.cars.values_mut() {
            if let Some(mut st) = car.drive.stunt.filter(|s| matches!(s.kind, StuntKind::Manoeuvre { .. })) {
                // [scene] A slow move at work, along its waypoints.
                let (pos, heading, v) = scene::step_manoeuvre(&mut st, &mut car.drive.path, car.pos, car.heading, dt);
                car.drive.stunt = Some(st);
                car.pos = pos;
                car.heading = heading;
                car.v = v;
                car.drive.poses.push((pos, heading));
                continue;
            }
            if let Some(mut st) = car.drive.stunt {
                let (pos, heading) = step_stunt(&mut st, car.pos, car.heading, dt);
                car.drive.stunt = Some(st);
                car.pos = pos;
                car.heading = heading;
                car.v = if st.is_donut() && st.settled { 6.0 } else { car.v };
                car.drive.poses.push((pos, heading));
                continue;
            }
            let lane_id = car.route[car.idx];
            if car.drive.lane == Some(lane_id) {
                car.drive.on_lane += dt;
            } else {
                car.drive.lane = Some(lane_id);
                car.drive.on_lane = 0.0;
            }
            let lane = &self.net.lanes[lane_id as usize];
            let (p, d) = lane.point_at(car.s);
            let right = Vec2::new(-d.y, d.x);
            // A wandering driver: a slow drift and a quicker twitch.
            let amp = car.drive.profile.weave_amplitude();
            let weave = if amp > 0.0 && car.v > 1.0 && !car.crashed() {
                let t = car.age * 0.45 + car.drive.profile.phase * 37.0;
                amp * (0.75 * (t * 0.9).sin() + 0.25 * (t * 2.3 + 1.7).sin())
            } else {
                car.drive.weave * (1.0 - dt).max(0.0)
            };
            car.drive.weave = weave;
            // [emergency] Plus an off-law move's swing out of the lane.
            let mut side = car.lateral + weave + car.drive.offlaw.side + car.drive.dodge;
            if lane.is_connector() && lane.turn != 0 {
                let swing = car.swing();
                if swing > 0.0 {
                    let k = (car.s / lane.length.max(0.1)).clamp(0.0, 1.0);
                    side -= lane.turn as f32 * swing * (k * std::f32::consts::PI).sin();
                }
            }
            car.pos = p + right * side;
            let mut heading = d.y.atan2(d.x);
            if car.glide_from != 0.0 && car.glide_len > 0.0 {
                // Nose along the glide: the slope of the eased offset.
                let u = (car.glide_done / car.glide_len).min(1.0);
                let slope = -car.glide_from * 0.5 * std::f32::consts::PI / car.glide_len * (std::f32::consts::PI * u).sin();
                heading += slope.atan();
            } else if car.lateral.abs() > 0.05 && car.v > 0.5 {
                // Nose turned toward the lane it is drifting into.
                heading += (-car.lateral.signum() * LC_LATERAL_SPEED).atan2(car.v.max(2.0));
            }
            if amp > 0.0 && car.v > 1.0 {
                let t = car.age * 0.45 + car.drive.profile.phase * 37.0;
                let dw = amp * 0.45 * (0.75 * 0.9 * (t * 0.9).cos() + 0.25 * 2.3 * (t * 2.3 + 1.7).cos());
                heading += dw.atan2(car.v.max(2.0));
            }
            if car.drive.offlaw.slew != 0.0 && car.v > 0.5 {
                heading += car.drive.offlaw.slew.atan2(car.v.max(2.0));
            }
            car.heading = heading;
            car.drive.poses.push((car.pos, heading));
        }
    }
}
