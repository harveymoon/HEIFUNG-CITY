//! [r10-worn-paths] The walkers' steps laid into the walking-heat field
//! (`crate::foot_wear`): once a tick, after everyone has moved.

use super::*;
use crate::foot_wear::{SIT_PER_TICK, STAND_PER_TICK};

impl World {
    /// Every step on foot at grade this tick (from `prev_pos` to `pos`)
    /// into the field; a little under people waiting at a stop or sitting
    /// on a bench. Someone up on a raised walkway or a deck leaves nothing
    /// on the ground below; a jump further than a few ticks' walk (a door,
    /// a ferry, a reload) only marks where they stand.
    pub(super) fn step_foot_wear(&mut self) {
        let reach = self.config.walk_speed * 3.0 + crate::foot_wear::CELL;
        let raised = !self.walk.raised.is_empty();
        let walk = &self.walk;
        let field = &mut self.foot_wear;
        for c in self.citizens.values() {
            if c.inside.is_some() {
                continue;
            }
            let on_foot = match &c.activity {
                Activity::Traveling { path, idx, .. } if c.mode == TravelMode::Walk => {
                    if let Some(&to) = path.get(*idx) {
                        if walk.deck_at_node(to) == Some(true) {
                            continue;
                        }
                        if raised && *idx > 0 && walk.level_on_leg(path[*idx - 1], to, c.pos) > 0.05 {
                            continue;
                        }
                    }
                    true
                }
                Activity::Passenger { seated: false, .. } => true,
                Activity::Waiting { .. } => {
                    field.add(c.pos, STAND_PER_TICK);
                    false
                }
                Activity::Sitting { .. } => {
                    field.add(c.pos, SIT_PER_TICK);
                    false
                }
                _ => false,
            };
            if !on_foot {
                continue;
            }
            let d = c.prev_pos.distance(c.pos);
            if d < 0.05 {
                continue;
            }
            if d > reach {
                field.add(c.pos, 1.0);
            } else {
                field.walk(c.prev_pos, c.pos, 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foot_wear::{FootWear, HALF_LIFE_HOURS};
    use crate::mind::IdleMind;

    fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
    }

    fn run(w: &mut World, ticks: usize) {
        let mut minds = IdleMind;
        for _ in 0..ticks {
            w.tick(&mut minds);
        }
    }

    /// A street with homes along it, lived in, everyone then kept indoors.
    fn town() -> World {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, social: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 160.0), LandUse::Residential);
        run(&mut w, 8 * 60);
        let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
        for id in sites {
            w.finish_construction(id);
        }
        let mut guard = 0;
        while w.citizens.len() < 6 && guard < 24 {
            run(&mut w, 60);
            guard += 1;
        }
        assert!(w.citizens.len() >= 6, "people moved in: {}", w.citizens.len());
        quiet(&mut w);
        w.foot_wear = FootWear::default();
        w
    }

    /// Everyone indoors, deciding nothing.
    fn quiet(w: &mut World) {
        for c in w.citizens.values_mut() {
            c.activity = Activity::AtHome;
            c.inside = c.home;
            c.next_decision_tick = u64::MAX;
        }
    }

    /// The first `n` citizens set walking to and fro across open ground
    /// south of the street (where no walkway runs: the field takes the
    /// steps wherever they are).
    fn pace(w: &mut World, n: usize, from: Vec2, to: Vec2) {
        let ids: Vec<CitizenId> = w.citizens.keys().take(n).collect();
        for (k, id) in ids.into_iter().enumerate() {
            w.peds.walkers.remove(&id);
            let c = &mut w.citizens[id];
            let off = Vec2::new(0.0, k as f32 * 0.3);
            let (a, b) = if k % 2 == 0 { (from, to) } else { (to, from) };
            let path: Vec<Vec2> = (0..40).map(|i| if i % 2 == 0 { b + off } else { a + off }).collect();
            c.inside = None;
            c.mode = TravelMode::Walk;
            c.pos = a + off;
            c.prev_pos = a + off;
            c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
            c.next_decision_tick = u64::MAX;
        }
    }

    #[test]
    fn heat_builds_where_people_walk_and_fades_when_they_stop() {
        let mut w = town();
        let (a, b) = (Vec2::new(-150.0, -60.0), Vec2::new(-90.0, -60.0));
        pace(&mut w, 4, a, b);
        run(&mut w, 120);
        let mid = Vec2::new(-120.0, -59.5);
        let hot = w.foot_wear.at(mid);
        assert!(hot > 10.0, "the walked line heats up: {hot}");
        assert_eq!(w.foot_wear.at(Vec2::new(-120.0, -90.0)), 0.0, "nobody walked here");
        assert_eq!(w.foot_wear.at(Vec2::new(60.0, -60.0)), 0.0, "nor here");
        // Everyone home: the line fades by half in the half-life.
        quiet(&mut w);
        let before = w.foot_wear.at(mid);
        run(&mut w, HALF_LIFE_HOURS as usize * 60);
        let after = w.foot_wear.at(mid);
        assert!(after < before * 0.55 && after > before * 0.45, "faded by half: {before} -> {after}");
    }

    #[test]
    fn foot_wear_is_deterministic() {
        let trace = || {
            let mut w = town();
            pace(&mut w, 5, Vec2::new(-150.0, -60.0), Vec2::new(-100.0, -20.0));
            run(&mut w, 200);
            ron::to_string(&w.foot_wear).unwrap()
        };
        let a = trace();
        assert!(a.len() > 20, "something was laid down: {a}");
        assert_eq!(a, trace());
    }

    #[test]
    fn an_old_save_loads_with_no_wear_and_a_save_keeps_it() {
        let mut w = town();
        pace(&mut w, 4, Vec2::new(-150.0, -60.0), Vec2::new(-90.0, -60.0));
        run(&mut w, 60);
        assert!(!w.foot_wear.is_empty());
        let text = ron::to_string(&w).unwrap();
        let back = World::from_ron(&text).unwrap();
        let mid = Vec2::new(-120.0, -59.5);
        let (a, b) = (w.foot_wear.at(mid), back.foot_wear.at(mid));
        assert!((a - b).abs() <= a * 0.11 + 0.1, "kept through a save: {a} vs {b}");
        // A save from before the field: the key is not there at all.
        let start = text.find("foot_wear:").expect("saved under its name");
        let ver = start + text[start..].find("version:").expect("its version");
        let end = ver + text[ver..].find(')').expect("end of the field") + 1;
        let end = end + usize::from(text[end..].starts_with(','));
        let old = format!("{}{}", &text[..start], &text[end..]);
        assert!(!old.contains("foot_wear"));
        let back = World::from_ron(&old).unwrap();
        assert!(back.foot_wear.is_empty(), "old saves start clean");
    }
}
