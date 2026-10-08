//! [fire-scene] Where fire engines stand at a fire, the second alarm, and
//! what the traffic is told to slow for and steer round (`Traffic::scenes`,
//! `traffic/scene.rs`).
//!
//! An engine is sent to the building's street, clear of the junctions at
//! either end (`SCENE_JUNCTION_CLEAR`), up whichever side is quicker, and
//! once there pulls in to the kerb on the building's side: its body flush with the edge of the road, nose the way it
//! came, so the crew climbs out on the pavement side and never stands in a
//! traffic lane. It used to stop where its route ended, in the middle of the
//! lane (sometimes in the junction), and the traffic drove through it. A fire
//! still burning hard (`SECOND_ALARM`) once the first engine is on its way
//! sounds a second alarm: the nearest other free engine comes too, with its
//! own crew and hose line, pulls in `SECOND_ENGINE_GAP` behind the first,
//! and the two knock the fire down twice as fast. Passing traffic slows to a
//! crawl past an engine at work and eases out round it.

use super::*;
use crate::services::*;
use crate::traffic::SceneZone;

impl World {
    /// [fire-scene] Where an engine pulls up for a fire in building `b`:
    /// on the street its lot fronts, on the building's side (a point in
    /// that side's lanes), clear of the junctions at either end.
    pub(super) fn fire_access(&self, b: BuildingId) -> Option<(SegmentId, Vec2)> {
        let lot = self.building_lot(b)?;
        let (seg, p) = lot.car_segment.map(|s| (s, lot.car_point))?;
        let (a, e) = self.roads.segment_points(seg);
        let len = a.distance(e);
        if len < 1.0 {
            return Some((seg, p));
        }
        let d = (e - a) / len;
        let clear = SCENE_JUNCTION_CLEAR.min(len * 0.5);
        let q = a + d * (p - a).dot(d).clamp(clear, len - clear);
        let perp = Vec2::new(-d.y, d.x);
        let side = if (lot.center - q).dot(perp) >= 0.0 { 1.0 } else { -1.0 };
        Some((seg, q + perp * side * crate::roads::LANE_WIDTH * 0.5))
    }

    /// [fire-scene] Pull fire engine `uid`, just arrived at the fire in
    /// `b`, in to the kerb on the building's side, clear of the junctions;
    /// a second engine stands behind the first (or ahead, with no room).
    pub(super) fn park_engine(&mut self, uid: UnitId, b: BuildingId, behind: Option<(Vec2, f32)>) {
        let Some(u) = self.services.units.get(uid) else { return };
        let (pos, heading) = (u.pos, u.heading);
        let toward = self.building_lot(b).map_or(pos, |l| l.center);
        let Some((seg, _, _)) = self.roads.nearest_street(pos) else { return };
        let s = self.roads.segment(seg);
        if !s.is_street() || s.lanes() == 0 {
            return;
        }
        let half = s.half_width();
        let (a, e) = self.roads.segment_points(seg);
        let len = a.distance(e);
        if len < 1.0 {
            return;
        }
        let mut d = (e - a) / len;
        // Nose the way it came along the street (up the far side, it
        // crosses over and stands nose against the flow at the building's
        // kerb, as engines do).
        let fwd = Vec2::new(heading.cos(), heading.sin());
        let (start, dir_sign) = if fwd.dot(d) >= 0.0 { (a, 1.0) } else { (e, -1.0) };
        d *= dir_sign;
        let clear = (SCENE_JUNCTION_CLEAR + ENGINE_HALF.0).min(len * 0.5);
        let mut t = (pos - start).dot(d);
        if let Some((first, _)) = behind {
            let ft = (first - start).dot(d);
            t = if ft - SECOND_ENGINE_GAP >= clear { ft - SECOND_ENGINE_GAP } else { ft + SECOND_ENGINE_GAP };
        }
        let t = t.clamp(clear, (len - clear).max(clear));
        let q = start + d * t;
        let perp = Vec2::new(-d.y, d.x);
        let side = if (toward - q).dot(perp) >= 0.0 { 1.0 } else { -1.0 };
        let at = q + perp * side * (half - ENGINE_HALF.1 - 0.1).max(0.0);
        let u = &mut self.services.units[uid];
        u.pos = at;
        u.heading = d.y.atan2(d.x);
    }

    /// [fire-scene] Sound the second alarm for fire `id` when it burns hard
    /// enough and its first engine is out: the nearest other free engine
    /// comes, lights and sirens. Once a fire.
    pub(super) fn maybe_second_alarm(&mut self, id: u32) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident(id) else { return };
        if inc.kind != IncidentKind::Fire || inc.outside || inc.second_alarm.is_some() || inc.intensity < SECOND_ALARM || !matches!(inc.stage, Stage::Dispatched | Stage::OnScene) {
            return;
        }
        let (first, at, b) = (inc.unit, inc.pos, inc.building);
        let Some(to) = self.fire_access(b) else { return };
        let out = self.units_out();
        let mut free: Vec<(f32, UnitId)> = self
            .services
            .units
            .iter()
            .filter(|(uid, u)| Some(*uid) != first && u.kind == UnitKind::FireEngine && self.unit_available(u, &out))
            .map(|(uid, u)| (u.car.and_then(|c| self.traffic.cars.get(c)).map_or(u.pos, |c| c.pos).distance(at), uid))
            .collect();
        free.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        // Sounded either way: with no engine free there is no second.
        if let Some(i) = self.services.incident_mut(id) {
            i.second_alarm = Some(tick);
        }
        for (_, uid) in free {
            if self.send_second(uid, id, to) {
                self.services.stats.second_alarms_today += 1;
                self.services.stats.second_alarms_total += 1;
                return;
            }
        }
    }

    /// [fire-scene] Engines working fire `id` now (on scene, standing).
    pub fn engines_on_scene(&self, id: u32) -> usize {
        self.services.units.values().filter(|u| u.kind == UnitKind::FireEngine && u.state == UnitState::OnScene && u.car.is_none() && u.incident == Some(id) && !u.leaving).count()
    }

    /// [fire-scene] What the traffic slows for and steers round this tick:
    /// fire engines standing at a fire.
    pub(super) fn set_traffic_scenes(&mut self) {
        let zones: Vec<SceneZone> = self
            .services
            .units
            .values()
            .filter(|u| u.kind == UnitKind::FireEngine && u.state == UnitState::OnScene && u.car.is_none())
            .map(|u| SceneZone::new(u.pos, u.heading, ENGINE_HALF.0, ENGINE_HALF.1))
            .collect();
        self.traffic.scenes = zones;
    }
}
