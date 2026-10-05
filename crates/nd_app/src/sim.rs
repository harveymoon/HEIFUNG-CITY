//! Owns the simulation and steps it on a fixed sim clock that is independent
//! of the frame rate.

use bevy::prelude::*;
use nd_minds::{HybridMind, MockBackend};
use nd_sim::{SimEvent, Vec2 as SVec2, World, WorldConfig};

use crate::camera::CameraMode;
use crate::ui::UiFlags;

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
    /// Ticks actually run per second of sim time asked for, smoothed over
    /// about half a second: 1.0 means the sim keeps up with the chosen
    /// speed, less means the tick budget is throttling it. Shown in the HUD.
    pub effective_speed: f32,
}

impl Sim {
    pub fn new() -> Self {
        let world = Self::fresh_world(nd_sim::LandscapeParams::default());
        Self {
            world,
            minds: HybridMind::new(Box::new(MockBackend::default())),
            speed_index: 1,
            paused: false,
            accumulator: 0.0,
            events: Vec::new(),
            full_rebuild: true,
            ticks_last_frame: 0,
            effective_speed: 1.0,
        }
    }

    /// A new world on a landscape, with the starter avenues.
    fn fresh_world(landscape: nd_sim::LandscapeParams) -> World {
        let mut world = World::new(WorldConfig { landscape, ..WorldConfig::default() });
        // Two starter avenues so the city has entrances from day one.
        world.add_road(&[SVec2::new(-700.0, 0.0), SVec2::new(700.0, 0.0)], 2, 2);
        world.add_road(&[SVec2::new(0.0, -700.0), SVec2::new(0.0, 700.0)], 2, 2);
        // The bridges the avenues need come with the map.
        world.economy.treasury = 60_000.0;
        world.economy.ledger = Default::default();
        world.drain_events();
        world
    }

    /// Throw the city away and start again on a new landscape.
    pub fn new_game(&mut self, landscape: nd_sim::LandscapeParams) {
        self.world = Self::fresh_world(landscape);
        self.events.clear();
        self.accumulator = 0.0;
        self.full_rebuild = true;
    }

    /// The demo district is laid out blind over the centre of the map, so
    /// the hotkey only fires on an empty city; the panel button may add it to
    /// an existing one deliberately.
    pub fn can_seed_by_hotkey(&self) -> bool {
        self.world.buildings.is_empty()
    }

    /// Lay out a small grid with mixed zoning around the starter avenues.
    pub fn seed_demo_district(&mut self) {
        use nd_sim::{Gesture, LandUse};
        self.world.start_project();
        for i in -2..=2 {
            let c = i as f32 * 140.0;
            self.world.add_gesture(Gesture::Road { points: vec![SVec2::new(-280.0, c), SVec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street });
            self.world.add_gesture(Gesture::Road { points: vec![SVec2::new(c, -280.0), SVec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street });
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
        // A branch off the trunk line (a turnout, eased into curves when built), running behind the southern row.
        self.world.add_gesture(Gesture::Road { points: vec![SVec2::new(-420.0, nd_sim::rail::TRUNK_Y), SVec2::new(-330.0, -410.0), SVec2::new(-280.0, -348.0), SVec2::new(-110.0, -348.0), SVec2::new(-70.0, -324.0), SVec2::new(150.0, -324.0)], lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: nd_sim::roads::RoadKind::Rail });
        self.world.implement();
        // The demo has the money for its showpieces.
        self.world.economy.treasury = self.world.economy.treasury.max(150_000.0);
        // A freight yard and a station set down beside the branch; their
        // street sides land on the southern avenue, so both open at once.
        if self.world.facilities.is_empty() {
            use nd_sim::RailFacility;
            let _ = self.world.place_rail_facility(RailFacility::FreightYard, SVec2::new(-200.0, -330.0));
            let _ = self.world.place_rail_facility(RailFacility::Station, SVec2::new(80.0, -310.0));
        }
        // A coal plant to start with: cheap power and a haze over the industrial corner.
        if self.world.buildings.values().any(|b| b.plant.is_some()) {
            return;
        }
        let lot = self
            .world
            .lots
            .iter()
            .filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.area() >= 600.0)
            .max_by(|a, b| a.1.area().total_cmp(&b.1.area()))
            .map(|(id, _)| id);
        if let Some(lot) = lot {
            let _ = self.world.build_power(lot, nd_sim::PowerPlantKind::Coal);
        }
        // A bus depot on industrial land and one line round the four central blocks.
        if self.world.transit.routes.is_empty() {
            let lot = self
                .world
                .lots
                .iter()
                .filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.car_access() && l.area() >= nd_sim::transit::DEPOT_MIN_AREA)
                .max_by(|a, b| a.1.area().total_cmp(&b.1.area()))
                .map(|(id, _)| id);
            if let Some(lot) = lot {
                let _ = self.world.build_depot(lot);
            }
            let line = self.world.add_bus_route();
            for p in [(-70.0, -136.0), (70.0, -136.0), (136.0, -70.0), (136.0, 70.0), (70.0, 136.0), (-70.0, 136.0), (-136.0, 70.0), (-136.0, -70.0)] {
                let _ = self.world.add_bus_stop(line, SVec2::new(p.0, p.1));
            }
            self.world.set_route_buses(line, 2);
        }
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

    /// Advance the world `n` ticks right now, paused or not: the remote API's
    /// `step`. The events stay in `World::events` for `step_sim` to drain
    /// this frame, so renderers see them exactly as they see a normal tick.
    pub fn step_ticks(&mut self, n: u32) {
        for _ in 0..n {
            self.world.tick(&mut self.minds);
        }
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
        app.insert_resource(Sim::new()).add_systems(Update, (sim_hotkeys.in_set(SimSet::Input), step_sim.in_set(SimSet::Step)));
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

/// Pause, speed and the demo seed. Hotkeys that drive the simulation itself
/// live here; the tool hotkeys live in `tools::tool_input`. Nothing fires
/// while an egui text field has the keyboard.
fn sim_hotkeys(keys: Res<ButtonInput<KeyCode>>, mut flags: ResMut<UiFlags>, mode: Res<CameraMode>, mut sim: ResMut<Sim>) {
    if flags.keyboard_captured {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        sim.paused = !sim.paused;
    }
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        sim.speed_index = (sim.speed_index + 1).min(SPEEDS.len() - 1);
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        sim.speed_index = sim.speed_index.saturating_sub(1);
    }
    // Seeding is irreversible (it implements straight away), so the key only
    // works on an empty map and never from the pavement; a city with
    // buildings needs the panel button. (No modifier chord: Ctrl+Shift+G is
    // taken by the system.)
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl && keys.just_pressed(KeyCode::KeyG) && *mode == CameraMode::Orbit {
        if sim.can_seed_by_hotkey() {
            sim.seed_demo_district();
            flags.message = "Demo district laid out around the centre".into();
        } else {
            flags.message = "The city already has buildings; use the Seed button in the panel to add the demo anyway.".into();
        }
    }
}

fn step_sim(time: Res<Time>, mut sim: ResMut<Sim>) {
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
    // Ticks delivered against ticks asked for, as a half-second moving
    // average: single frames carry 0 or 1 tick at low speeds, so the raw
    // ratio is meaningless without smoothing.
    let dt = time.delta_secs();
    if !sim.paused && dt > 0.0 {
        let ratio = ticks as f32 / (dt * sim.speed());
        let k = 1.0 - (-dt / 0.5).exp();
        sim.effective_speed += (ratio - sim.effective_speed) * k;
    }
    let events = sim.world.drain_events();
    sim.events = events;
}
