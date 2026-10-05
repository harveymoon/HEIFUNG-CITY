//! Owns the simulation and steps it on a fixed sim clock that is independent
//! of the frame rate.

use bevy::prelude::*;
use nd_minds::{HybridMind, MockBackend};
use nd_sim::{SimEvent, Vec2 as SVec2, World, WorldConfig};

/// Sim minutes per real second for each speed step.
pub const SPEEDS: [(&str, f32); 5] = [("1×", 1.0), ("4×", 4.0), ("16×", 16.0), ("64×", 64.0), ("256×", 256.0)];
const MAX_TICKS_PER_FRAME: u32 = 400;
/// Real time the sim may consume per frame before it yields to rendering.
const TICK_BUDGET: std::time::Duration = std::time::Duration::from_millis(12);

#[derive(Resource)]
pub struct Sim {
    pub world: World,
    pub minds: HybridMind,
    pub speed_index: usize,
    pub paused: bool,
    accumulator: f32,
    /// Events produced since the previous frame.
    pub events: Vec<SimEvent>,
    /// Set by load; render systems rebuild everything.
    pub full_rebuild: bool,
    pub ticks_last_frame: u32,
}

impl Sim {
    pub fn new() -> Self {
        let mut world = World::new(WorldConfig::default());
        // Two starter avenues so the city has entrances from day one.
        world.add_road(&[SVec2::new(-700.0, 0.0), SVec2::new(700.0, 0.0)], 2, 2);
        world.add_road(&[SVec2::new(0.0, -700.0), SVec2::new(0.0, 700.0)], 2, 2);
        world.drain_events();
        Self {
            world,
            minds: HybridMind::new(Box::new(MockBackend::default())),
            speed_index: 1,
            paused: false,
            accumulator: 0.0,
            events: Vec::new(),
            full_rebuild: true,
            ticks_last_frame: 0,
        }
    }

    /// Lay out a small grid with mixed zoning around the starter avenues.
    pub fn seed_demo_district(&mut self) {
        use nd_sim::{Gesture, LandUse};
        self.world.start_project();
        for i in -2..=2 {
            let c = i as f32 * 140.0;
            self.world.add_gesture(Gesture::Road { points: vec![SVec2::new(-280.0, c), SVec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false });
            self.world.add_gesture(Gesture::Road { points: vec![SVec2::new(c, -280.0), SVec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false });
        }
        let cell = |cx: f32, cy: f32| {
            vec![SVec2::new(cx - 65.0, cy - 65.0), SVec2::new(cx + 65.0, cy - 65.0), SVec2::new(cx + 65.0, cy + 65.0), SVec2::new(cx - 65.0, cy + 65.0)]
        };
        let plan = [
            (-210.0, -210.0, LandUse::Industrial), (-70.0, -210.0, LandUse::Industrial), (70.0, -210.0, LandUse::Residential), (210.0, -210.0, LandUse::Residential),
            (-210.0, -70.0, LandUse::Commercial), (-70.0, -70.0, LandUse::Entertainment), (70.0, -70.0, LandUse::Commercial), (210.0, -70.0, LandUse::Residential),
            (-210.0, 70.0, LandUse::Residential), (-70.0, 70.0, LandUse::Park), (70.0, 70.0, LandUse::Civic), (210.0, 70.0, LandUse::Commercial),
            (-210.0, 210.0, LandUse::Residential), (-70.0, 210.0, LandUse::Residential), (70.0, 210.0, LandUse::Entertainment), (210.0, 210.0, LandUse::Residential),
        ];
        for (x, y, lu) in plan {
            self.world.add_gesture(Gesture::Zone { polygon: cell(x, y), land_use: lu });
        }
        self.world.implement();
    }

    /// Fraction of the way from the last tick to the next, for interpolation.
    pub fn frac(&self) -> f32 {
        if self.paused {
            1.0
        } else {
            self.accumulator.clamp(0.0, 1.0)
        }
    }

    pub fn speed(&self) -> f32 {
        SPEEDS[self.speed_index].1
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        let text = self.world.to_ron().map_err(|e| e.to_string())?;
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, text).map_err(|e| e.to_string())
    }

    pub fn load(&mut self, path: &str) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        self.world = World::from_ron(&text).map_err(|e| e.to_string())?;
        self.full_rebuild = true;
        Ok(())
    }
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Sim::new()).add_systems(Update, step_sim.in_set(SimSet::Step));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimSet {
    /// Player tools mutate the world.
    Input,
    /// The world advances.
    Step,
    /// Renderers react to the new state.
    Sync,
}

fn step_sim(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut sim: ResMut<Sim>) {
    if keys.just_pressed(KeyCode::Space) {
        sim.paused = !sim.paused;
    }
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        sim.speed_index = (sim.speed_index + 1).min(SPEEDS.len() - 1);
    }
    if keys.just_pressed(KeyCode::KeyG) {
        sim.seed_demo_district();
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        sim.speed_index = sim.speed_index.saturating_sub(1);
    }

    let mut ticks = 0;
    if !sim.paused {
        let speed = sim.speed();
        sim.accumulator += time.delta_secs() * speed;
        let s = &mut *sim;
        let start = std::time::Instant::now();
        while s.accumulator >= 1.0 && ticks < MAX_TICKS_PER_FRAME {
            s.world.tick(&mut s.minds);
            s.accumulator -= 1.0;
            ticks += 1;
            if start.elapsed() > TICK_BUDGET {
                break;
            }
        }
        // Drop any backlog we could not afford: the sim runs slower, the frame stays smooth.
        if s.accumulator > 2.0 {
            s.accumulator = 1.0;
        }
    }
    sim.ticks_last_frame = ticks;
    let events = sim.world.drain_events();
    sim.events = events;
}
