//! [dams] (H1) The hydro dam wall in the world: where a wall may go (bank
//! to bank across the plant's river, one end by the plant, clear of every
//! road), what it costs and what the plant then makes, building it with
//! its footpath, and the rule that no road runs over a dam. Data in
//! `crate::dams`.

use super::*;
use crate::dams::{self as dm, DamPlan, DamWall};
use crate::geom::{distance_to_outline, segment_distance_between};

impl World {
    /// The hydro plant `plant` and its lot, or why not.
    fn hydro_plant(&self, plant: BuildingId) -> Result<(&Building, &Lot), String> {
        let b = self.buildings.get(plant).ok_or("no such building")?;
        if b.plant != Some(PowerPlantKind::Hydro) {
            return Err(format!("{} is not a hydro plant: only a hydro plant has a dam", b.name));
        }
        let lot = self.lots.get(b.lot).ok_or("the plant has no lot")?;
        Ok((b, lot))
    }

    /// Why a wall cannot end at `p`, if so: it must stand on dry ground on
    /// a bank. (The tool checks the first click with this.)
    pub fn dam_end_problem(&self, p: Vec2) -> Option<String> {
        if self.landscape.is_water(p) {
            return Some("a dam wall ends on the bank, not in the water: click the dry ground at the water's edge".into());
        }
        if self.landscape.water_distance(p) > dm::DAM_BANK_REACH {
            return Some(format!("too far from the water: a dam wall ends within {:.0} m of the bank", dm::DAM_BANK_REACH));
        }
        None
    }

    /// What a dam wall from `a` to `b` for `plant` would be: its length,
    /// the water it spans, its cost and the power the plant would make. Or
    /// why it cannot be built: both ends on the banks of the plant's river,
    /// one of them within `DAM_NEAR_PLANT` of the plant, roughly square to
    /// the flow, across the water once, clear of every road, rail and other
    /// dam, and money in the treasury.
    pub fn plan_dam_wall(&self, plant: BuildingId, a: Vec2, b: Vec2) -> Result<DamPlan, String> {
        let (bld, lot) = self.hydro_plant(plant)?;
        if self.landscape.is_flat() {
            return Err("there is no river on this map".into());
        }
        let (ri, _, _) = self.landscape.river_beside(&lot.polygon, 30.0).ok_or("the plant has no river beside it")?;
        let river = &self.landscape.rivers[ri];
        for p in [a, b] {
            if let Some(why) = self.dam_end_problem(p) {
                return Err(why);
            }
        }
        let length = a.distance(b);
        if length < 4.0 {
            return Err("click the far bank to draw the wall".into());
        }
        if length > dm::DAM_MAX_LENGTH {
            return Err(format!("the wall would be {length:.0} m long: {:.0} m at most (find a narrower place)", dm::DAM_MAX_LENGTH));
        }
        let near = |p: Vec2| if point_in_polygon(p, &lot.polygon) { 0.0 } else { distance_to_outline(p, &lot.polygon) };
        if near(a).min(near(b)) > dm::DAM_NEAR_PLANT {
            return Err(format!("one end of the wall must be within {:.0} m of {}", dm::DAM_NEAR_PLANT, bld.name));
        }
        // Across the water once: dry, wet, dry.
        let steps = (length / 2.0).ceil().max(2.0) as usize;
        let (mut stretches, mut wet, mut span) = (0, false, 0.0f32);
        for k in 0..=steps {
            let now = self.landscape.is_water(a.lerp(b, k as f32 / steps as f32));
            if now {
                span += length / steps as f32;
            }
            if now && !wet {
                stretches += 1;
            }
            wet = now;
        }
        if stretches == 0 {
            return Err("the wall must cross the river: click one bank, then the other".into());
        }
        if stretches > 1 {
            return Err("the wall would cross the water more than once: draw it straight across from bank to bank".into());
        }
        // Across the plant's own river, and about square to its flow.
        // (Touching a vertex of the centre line counts: the nearest stretch.)
        let crossing = river.points.windows(2).enumerate().map(|(i, w)| (i, segment_distance_between(a, b, w[0], w[1]))).filter(|(_, d)| *d < 0.5).min_by(|x, y| x.1.total_cmp(&y.1)).map(|(i, _)| i);
        let Some(seg) = crossing else {
            return Err(format!("the wall must cross the river beside {} (the one it takes its water from)", bld.name));
        };
        let flow = river.flow(seg);
        let dir = (b - a) / length;
        if dir.dot(flow).abs() > dm::DAM_MAX_SKEW {
            return Err("the wall runs too much along the river: draw it straight across".into());
        }
        let upstream = -flow;
        let probe = DamWall { plant, a, b, upstream, river: ri, path: None };
        let crest = probe.crest(10);
        // No dam on a road (or a railway, or a footpath already there).
        let pad = Vec2::splat(probe.bow() + 30.0);
        let (lo, hi) = (a.min(b) - pad, a.max(b) + pad);
        for sid in self.roads.segments_in_box(lo, hi) {
            let s = self.roads.segment(sid);
            if s.tunnel {
                continue;
            }
            let (p, q) = self.roads.segment_points(sid);
            if crest.windows(2).any(|w| segment_distance_between(w[0], w[1], p, q) < s.half_width() + dm::DAM_HALF_WIDTH) {
                let what = if s.is_rail() {
                    "the railway".to_string()
                } else if let Some(st) = self.streets.of(sid) {
                    st.name.clone()
                } else if s.kind == RoadKind::Path {
                    "a footpath".to_string()
                } else {
                    "a road".to_string()
                };
                return Err(format!("the wall would run over {what}: a dam may not be built on a road (move it, or take the road away first)"));
            }
        }
        for w in self.dams.walls.iter().filter(|w| w.plant != plant) {
            if segment_distance_between(a, b, w.a, w.b) < dm::DAM_SPACING {
                let name = self.buildings.get(w.plant).map_or("another plant", |b| b.name.as_str());
                return Err(format!("too close to the dam of {name}"));
            }
        }
        let cost = (length * dm::DAM_COST_PER_M).round();
        if self.economy.treasury < cost {
            return Err(format!("the treasury has {:.0}; this wall costs {cost:.0}", self.economy.treasury));
        }
        let site = self.power_site(bld.lot, PowerPlantKind::Hydro).unwrap_or(0.0);
        let expected_mw = self.plant_capacity_mw(bld) * site;
        Ok(DamPlan { plant, a, b, upstream, river: ri, length, span, cost, expected_mw })
    }

    /// Build the wall `plan_dam_wall` describes, paying for it, with a
    /// footpath along its crest. A wall drawn for a plant that had one (or
    /// the old automatic arch) takes its place.
    pub fn build_dam_wall(&mut self, plant: BuildingId, a: Vec2, b: Vec2) -> Result<DamPlan, String> {
        let plan = self.plan_dam_wall(plant, a, b)?;
        self.economy.debit_item(Flow::Construction, plan.cost, Item::Named("dam wall".to_string()));
        self.remove_dam_wall(plant);
        let mut wall = DamWall { plant, a: plan.a, b: plan.b, upstream: plan.upstream, river: plan.river, path: None };
        // Dams carry footpaths only: one along the crest, from bank to bank
        // (from where it leaves the plant's own ground, which a path may
        // not cross).
        let mut pts = wall.crest(12);
        if let Some(poly) = self.buildings.get(plant).and_then(|b| self.lots.get(b.lot)).map(|l| l.polygon.clone()) {
            while pts.len() > 2 && point_in_polygon(pts[0], &poly) {
                pts.remove(0);
            }
            while pts.len() > 2 && point_in_polygon(pts[pts.len() - 1], &poly) {
                pts.pop();
            }
        }
        // The walkway is part of the wall: what the path's works would
        // charge is in the wall's price already.
        let treasury = self.economy.treasury;
        let id = self.add_path(&pts);
        let bridge = treasury - self.economy.treasury;
        if bridge > 0.0 {
            self.economy.credit_item(Flow::Construction, bridge, Item::Named("bridges".into()));
        }
        if self.master.gestures.contains_key(&id) {
            wall.path = Some(id);
        }
        self.dams.walls.push(wall);
        self.dams.pending.retain(|p| *p != plant);
        self.dams.version += 1;
        self.refresh_power();
        Ok(plan)
    }

    /// A hydro plant just placed with the Buildings tool waits for its
    /// wall: it makes nothing, and draws no dam, until the wall is drawn.
    pub fn await_dam_wall(&mut self, plant: BuildingId) {
        if self.hydro_plant(plant).is_ok() && self.dams.wall_of(plant).is_none() && !self.dams.is_pending(plant) {
            self.dams.pending.push(plant);
            self.dams.version += 1;
            self.refresh_power();
        }
    }

    /// Take a plant's wall away (its footpath stays a path).
    pub(super) fn remove_dam_wall(&mut self, plant: BuildingId) {
        let n = self.dams.walls.len();
        self.dams.walls.retain(|w| w.plant != plant);
        if self.dams.walls.len() != n {
            self.dams.version += 1;
        }
    }

    /// A plant pulled down takes its dam (and its place in the queue) with it.
    pub(super) fn forget_dam(&mut self, plant: BuildingId) {
        self.remove_dam_wall(plant);
        let n = self.dams.pending.len();
        self.dams.pending.retain(|p| *p != plant);
        if self.dams.pending.len() != n {
            self.dams.version += 1;
        }
    }

    /// Share of its output a plant's dam allows: none while a hydro plant
    /// waits for its wall, all of it otherwise.
    pub(super) fn dam_share(&self, b: &Building) -> f32 {
        if b.plant != Some(PowerPlantKind::Hydro) || self.dams.pending.is_empty() {
            return 1.0;
        }
        match self.lots.get(b.lot).and_then(|l| l.building) {
            Some(id) if self.dams.is_pending(id) => 0.0,
            _ => 1.0,
        }
    }

    /// Where the old automatic arch of a plant without a drawn wall
    /// stands: (centre on the river, flow, river width). None for a plant
    /// with a wall drawn or waiting for one.
    pub fn auto_dam(&self, plant: BuildingId) -> Option<(Vec2, Vec2, f32)> {
        let (_, lot) = self.hydro_plant(plant).ok()?;
        if self.dams.wall_of(plant).is_some() || self.dams.is_pending(plant) {
            return None;
        }
        let (ri, _, bank) = self.landscape.river_beside(&lot.polygon, 40.0)?;
        let r = &self.landscape.rivers[ri];
        let (seg, c, _) = r.nearest(bank);
        Some((c, r.flow(seg), r.width))
    }

    /// No road on a dam: why a planned road (`pts` at `levels`, a street,
    /// highway or railway `half_width` wide; footpaths may cross) cannot
    /// be built, if it would run over a dam wall. Tunnels pass under.
    pub fn road_over_dam(&self, pts: &[Vec2], levels: &[f32], kind: RoadKind, half_width: f32) -> Option<String> {
        if kind == RoadKind::Path || self.dams.walls.is_empty() {
            return None;
        }
        let deep = |i: usize| levels.get(i).is_some_and(|l| *l <= crate::roads::TUNNEL_LEVEL);
        for wall in &self.dams.walls {
            let crest = wall.crest(10);
            for (i, w) in pts.windows(2).enumerate() {
                if deep(i) && deep(i + 1) {
                    continue;
                }
                if crest.windows(2).any(|c| segment_distance_between(c[0], c[1], w[0], w[1]) < half_width + dm::DAM_HALF_WIDTH) {
                    let name = self.buildings.get(wall.plant).map_or("a hydro plant", |b| b.name.as_str());
                    return Some(format!("Not implemented: a road would run over the dam of {name}. Dams carry footpaths only: take the road across somewhere else."));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::{LandscapeParams, LandscapePreset};

    /// A town on a river map with a hydro plant beside the river; the
    /// plant, and a point on each bank square across from it.
    fn river_town() -> Option<(World, BuildingId, Vec2, Vec2)> {
        for seed in 1..40u64 {
            let mut cfg = WorldConfig::test_town();
            cfg.landscape = LandscapeParams::preset(LandscapePreset::RiverValley, seed);
            cfg.emergencies = false;
            cfg.society = false;
            cfg.social = false;
            let mut w = World::new(cfg);
            w.economy.treasury = 1.0e7;
            let Some(ri) = w.landscape.rivers.iter().position(|r| r.current >= crate::landscape::MIN_HYDRO_CURRENT && !r.stream) else { continue };
            let r = w.landscape.rivers[ri].clone();
            let mid = r.points.len() / 2;
            let (c, flow) = (r.points[mid], r.flow(mid));
            let across = Vec2::new(-flow.y, flow.x);
            let near = c + across * (r.width * 0.5 + 18.0);
            if w.landscape.is_water(near) || near.length() > 700.0 {
                continue;
            }
            // A street along the bank, zoned, then the plant on a lot by the water.
            let along = near + across * 30.0;
            w.add_road(&[along - flow * 120.0, along + flow * 120.0], 1, 1);
            w.add_zone(vec![along - flow * 60.0 - across * 28.0, along + flow * 60.0 - across * 28.0, along + flow * 60.0, along - flow * 60.0], LandUse::Industrial);
            let Ok(id) = w.place_building(crate::Placeable::Plant(PowerPlantKind::Hydro), near + across * 6.0) else { continue };
            let lot = &w.lots[w.buildings[id].lot];
            if w.landscape.river_beside(&lot.polygon, 30.0).is_none_or(|(i, ..)| i != ri) {
                continue;
            }
            let mut a = c + across * (r.width * 0.5 + 4.0);
            while w.landscape.is_water(a) {
                a += across * 2.0;
            }
            let mut b = c - across * (r.width * 0.5 + 4.0);
            while w.landscape.is_water(b) {
                b -= across * 2.0;
            }
            return Some((w, id, a, b));
        }
        None
    }

    #[test]
    fn dam_wall_spans_the_river_and_powers_the_plant() {
        let Some((mut w, plant, a, b)) = river_town() else { panic!("no river town found") };
        // Placed with the tool, the plant waits for its wall and makes nothing.
        w.await_dam_wall(plant);
        assert!(w.dams.is_pending(plant));
        assert!(w.auto_dam(plant).is_none(), "no automatic arch while the wall is awaited");
        let plant_b = w.buildings[plant].clone();
        assert_eq!(w.dam_share(&plant_b), 0.0);
        assert!(w.plan_dam_wall(plant, a, a.lerp(b, 0.5)).is_err(), "an end in the water");
        let plan = w.plan_dam_wall(plant, a, b).expect("a wall bank to bank");
        assert!(plan.length > 0.0 && plan.cost > 0.0 && plan.span > 0.0 && plan.span < plan.length);
        assert!(plan.expected_mw > 0.0, "{plan:?}");
        let before = w.economy.treasury;
        w.build_dam_wall(plant, a, b).expect("built");
        assert!((before - w.economy.treasury - plan.cost).abs() < 1.0);
        assert!(!w.dams.is_pending(plant));
        let wall = w.dams.wall_of(plant).expect("the wall").clone();
        assert_eq!(w.dam_share(&w.buildings[plant].clone()), 1.0);
        // Its crest carries a footpath, nothing else.
        let path = wall.path.expect("a footpath on the crest");
        assert!(w.master.gestures.contains_key(&path));
        // A street across the dam is refused on implement; a footpath is not.
        let mid = wall.crest(2)[1];
        let flow = -wall.upstream;
        let road = [mid - flow * 40.0, mid + flow * 40.0];
        assert!(w.road_over_dam(&road, &[0.0, 0.0], RoadKind::Street, 4.0).is_some());
        assert!(w.road_over_dam(&road, &[0.0, 0.0], RoadKind::Path, 1.5).is_none());
        w.add_gesture(crate::plan::Gesture::Road { points: road.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        let refusal = w.try_implement().expect_err("no road on a dam");
        assert!(refusal.contains("dam"), "{refusal}");
        // Bulldozing the plant takes the wall with it.
        w.project = None;
        w.bulldoze_building(plant).expect("bulldozed");
        assert!(w.dams.wall_of(plant).is_none());
    }

    #[test]
    fn dam_wall_may_not_go_over_a_road() {
        let Some((mut w, plant, a, b)) = river_town() else { panic!("no river town found") };
        // A footbridge across the river right where the wall would go.
        let out = (b - a).normalize();
        let id = w.add_path(&[a.lerp(b, 0.15), b + out * 12.0]);
        assert!(w.master.gestures.contains_key(&id), "the footbridge is built: {:?}", w.implement_refused);
        let why = w.plan_dam_wall(plant, a, b).expect_err("over a road");
        assert!(why.contains("may not be built on a road"), "{why}");
    }
}
