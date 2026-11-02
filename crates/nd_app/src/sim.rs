//! Owns the simulation and steps it on a fixed sim clock that is independent
//! of the frame rate.

use std::path::Path;

use bevy::prelude::*;
use nd_minds::{HybridMind, MockBackend};
use nd_sim::{SimEvent, World, WorldConfig};

use crate::camera::CameraMode;
use crate::ui::UiFlags;

/// Sim minutes per real second for each speed step.
/// Game speeds as multiples of real time for what you see move: at 1×
/// people walk and cars drive at their true speed (one tick, a clock
/// minute, shows `traffic_seconds_per_tick` of motion, so 1× is a tick
/// every few real seconds). The clock itself runs faster than real life
/// by 60 / traffic_seconds_per_tick (15× today): that is what lets a day's
/// commute fit in a believable stretch of it.
pub const SPEEDS: [(&str, f32); 6] = [("1×", 1.0), ("4×", 4.0), ("16×", 16.0), ("64×", 64.0), ("256×", 256.0), ("1024×", 1024.0)];
/// The speed a new game starts at (16×: the pace the game always had).
pub const DEFAULT_SPEED: usize = 2;
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
    /// Counts loads and new games, so state outside the sim (the remote
    /// API's follow and history) can tell the city was replaced, whoever
    /// replaced it.
    pub generation: u64,
    /// [fix/review-app] The tick this city was last saved, loaded or
    /// started at (None: never). See [`Sim::unsaved_play`].
    pub saved_tick: Option<u64>,
    /// The plan's gesture count when this city started or was loaded: a
    /// city still at it, with no building and no project, is untouched.
    start_gestures: usize,
    pub ticks_last_frame: u32,
    /// Milliseconds the last frame spent ticking (the graphics throttler
    /// holds its tier when the sim, not the renderer, fills the frame).
    pub tick_ms: f32,
    /// Ticks actually run per second of sim time asked for, smoothed over
    /// about half a second: 1.0 means the sim keeps up with the chosen
    /// speed, less means the tick budget is throttling it. Shown in the HUD.
    pub effective_speed: f32,
    /// Every building's resolved style (`nd_procgen::StyleBook`): filled by
    /// the renderer as it draws, read by the inspector and the remote API so
    /// the style they name is the one on screen.
    pub styles: nd_procgen::StyleBook,
    /// [fire-scene] Every building's drawn height, filled by the renderer
    /// the same way (`fire_scene::BuildingTops`).
    pub building_tops: crate::fire_scene::BuildingTops,
    /// [K34] Each depot's slots for its real boxes (from its drawn style,
    /// `Geometry::box_slots`), with the version they were drawn at; and a
    /// count bumped whenever they change.
    pub depot_slots: std::collections::HashMap<nd_sim::BuildingId, (u64, Vec<nd_procgen::BoxSlot>)>,
    pub depot_slots_version: u64,
    /// [r8] Each depot's painted lorry bays (from its drawn style): the
    /// middle of each bay on the ground and the way a lorry in it faces.
    pub depot_bays: std::collections::HashMap<nd_sim::BuildingId, Vec<(bevy::math::Vec3, f32)>>,
    /// [K34] The real boxes on screen (depots and trains), for picking:
    /// record, floor centre, rotation, forty-footer.
    pub box_spots: Vec<(nd_sim::containers::BoxId, bevy::math::Vec3, bevy::math::Quat, bool)>,
}

impl Sim {
    /// [r8] Where a lorry standing in a depot's bay is drawn: the bay's
    /// middle (the rig's centre), its heading and the ground there. None
    /// for a lorry not in a bay, or a bay the depot does not draw (it is
    /// then drawn where the road has it, at the kerb).
    pub fn bay_pose(&self, car: nd_sim::CarId) -> Option<(bevy::math::Vec2, f32, f32)> {
        if !self.world.traffic.cars.get(car).is_some_and(|c| c.bay) {
            return None;
        }
        let (d, k) = self.world.lorry_bay(car)?;
        let &(at, yaw) = self.depot_bays.get(&d)?.get(k)?;
        Some((bevy::math::Vec2::new(at.x, at.z), yaw, at.y))
    }

    pub fn new() -> Self {
        let world = Self::fresh_world(nd_sim::LandscapeParams::default());
        let (start, start_gestures) = (world.clock.tick, world.master.gestures.len());
        Self {
            world,
            minds: HybridMind::new(Box::new(MockBackend::default())),
            speed_index: DEFAULT_SPEED,
            paused: false,
            accumulator: 0.0,
            events: Vec::new(),
            full_rebuild: true,
            generation: 0,
            saved_tick: Some(start),
            start_gestures,
            ticks_last_frame: 0,
            tick_ms: 0.0,
            effective_speed: 1.0,
            styles: nd_procgen::StyleBook::default(),
            building_tops: Default::default(),
            depot_slots: Default::default(),
            depot_slots_version: 0,
            depot_bays: Default::default(),
            box_spots: Vec::new(),
        }
    }

    /// A new world on a landscape: the highway and the trunk line from
    /// elsewhere to elsewhere and nothing else; the player builds the rest
    /// (and joins it to the highway so people can move in).
    fn fresh_world(mut landscape: nd_sim::LandscapeParams) -> World {
        // Every new game has its highway.
        landscape.highway = true;
        let mut world = World::new(WorldConfig { landscape, ..WorldConfig::default() });
        world.economy.treasury = 60_000.0;
        world.economy.ledger = Default::default();
        world.drain_events();
        // [identity] Every new game is a game of its own, unnamed until
        // the player names it.
        world.identity = nd_sim::CityIdentity { name: String::new(), game_id: crate::ui::saves::new_game_id() };
        world
    }

    /// [identity] Name the city (tidied, see `nd_sim::identity::clean_name`).
    pub fn set_city_name(&mut self, name: &str) -> String {
        crate::ui::saves::rename_city(&mut self.world, name)
    }

    /// Throw the city away and start again on a new landscape.
    pub fn new_game(&mut self, landscape: nd_sim::LandscapeParams) {
        self.world = Self::fresh_world(landscape);
        // Answers still out are about the old city's citizens.
        nd_sim::MindProvider::reset(&mut self.minds);
        self.events.clear();
        self.accumulator = 0.0;
        self.city_replaced();
    }

    /// Throw the city away and start again with a whole example city
    /// generated on `landscape` (`nd_procgen::citygen`): its streets, its
    /// zoning, its interchanges and a power plant, ready to grow.
    pub fn generate_city(&mut self, preset: nd_procgen::citygen::CityPreset, landscape: nd_sim::LandscapeParams) -> (nd_procgen::citygen::CityPlan, nd_procgen::citygen::LaidCity) {
        self.new_game(landscape);
        let seed = self.world.landscape.params.seed;
        let plan = nd_procgen::citygen::generate(preset, seed, &self.world.landscape);
        let laid = nd_procgen::citygen::lay_city(&mut self.world, &plan);
        (plan, laid)
    }

    /// The demo district is laid out blind over the centre of the map, so
    /// the hotkey only fires on an empty city; the panel button may add it to
    /// an existing one deliberately.
    pub fn can_seed_by_hotkey(&self) -> bool {
        self.world.buildings.is_empty()
    }

    /// Lay out a small grid with mixed zoning in the middle of the map,
    /// joined to the highway by a street and a pair of ramps.
    pub fn seed_demo_district(&mut self) {
        nd_sim::scenarios::demo_district(&mut self.world);
    }

    /// Fraction of the way from the last tick to the next, for interpolation.
    pub fn frac(&self) -> f32 {
        if self.paused {
            1.0
        } else {
            self.accumulator.clamp(0.0, 1.0)
        }
    }

    /// Ticks per real second at the chosen speed: the real-time multiple
    /// over the seconds of motion one tick shows.
    pub fn speed(&self) -> f32 {
        SPEEDS[self.speed_index].1 / self.world.config.traffic_seconds_per_tick.max(0.1)
    }

    /// Advance the world `n` ticks right now, paused or not: the remote API's
    /// `step`. The events stay in `World::events` for `step_sim` to drain
    /// this frame, so renderers see them exactly as they see a normal tick.
    pub fn step_ticks(&mut self, n: u32) {
        for _ in 0..n {
            self.world.tick(&mut self.minds);
        }
    }

    /// Write the world as RON, atomically (see [`write_atomic`]).
    pub fn save(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        save_world(&self.world, path.as_ref())?;
        self.saved_tick = Some(self.world.clock.tick);
        Ok(())
    }

    /// [fix/review-app] The city has play time that no save holds: its
    /// clock has run since it was last saved, loaded or started (the
    /// autosave does not count: the next day overwrites it). The remote API
    /// asks for `confirm` before replacing such a city. Edits made while
    /// paused do not count.
    pub fn unsaved_play(&self) -> bool {
        self.saved_tick != Some(self.world.clock.tick) && !self.untouched()
    }

    /// Nothing of the player's in it: no building, no project, no gesture
    /// beyond what the city started with (an empty landscape left running
    /// at the start, say). Replacing it loses nothing.
    pub fn untouched(&self) -> bool {
        let w = &self.world;
        w.buildings.is_empty() && !w.has_project() && w.master.gestures.len() <= self.start_gestures
    }

    pub fn load(&mut self, path: &str) -> Result<(), String> {
        crate::crash::note_io("loading", Path::new(path));
        // [saves] Compressed or plain RON (saves from before compression).
        self.world = World::read_save(path)?;
        crate::crash::note_io("loaded", Path::new(path));
        self.city_replaced();
        self.saved_tick = Some(self.world.clock.tick);
        Ok(())
    }

    /// A different city now runs: renderers rebuild, and anything keyed by
    /// the old world's ids starts over. The minds' pending answers name
    /// old citizens whose keys the new world reuses, so they go too (the
    /// same backend `new` starts with).
    fn city_replaced(&mut self) {
        self.full_rebuild = true;
        self.generation += 1;
        // A new city starts with nothing to lose (a load sets it again).
        self.saved_tick = Some(self.world.clock.tick);
        self.start_gestures = self.world.master.gestures.len();
        self.minds = HybridMind::new(Box::new(MockBackend::default()));
    }
}

/// Serialise `world` to `path`, atomically.
///
/// [memory] Streamed straight into the file: a big city's save is well over
/// 100 MB, and building it as one `String` first asked for a single 256 MiB
/// block (the crash at day 10 of a 2,000-lot city).
///
/// [saves] Gzip-compressed RON (`World::write_ron_gz`): a tenth of the size
/// and of the disk writes. Loading takes either form, so older saves load.
pub fn save_world(world: &World, path: &Path) -> Result<(), String> {
    write_atomic_with(path, |out| world.write_ron_gz(out)).map_err(|e| e.to_string())
}

/// Copy `from` to `to` atomically (as [`write_atomic`]): the autosave's
/// second copy, written once and copied instead of serialised twice.
pub fn copy_atomic(from: &Path, to: &Path) -> std::io::Result<()> {
    write_atomic_with(to, |out| std::io::copy(&mut std::fs::File::open(from)?, out).map(|_| ()))
}

/// Write `bytes` to a temporary file beside `path`, flush it to disk, then
/// rename it over `path`: a crash or a full disk mid-write leaves the old
/// file whole instead of a truncated one. (`rename` replaces an existing
/// file on Windows too.) The temporary name ends in `.tmp`, so save
/// listings, which look for `.ron`, never show it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    write_atomic_with(path, |out| out.write_all(bytes))
}

/// [`write_atomic`] with the bytes written by `fill` into a buffered
/// writer on the temporary file, so a big save never sits whole in memory.
pub fn write_atomic_with(path: &Path, fill: impl FnOnce(&mut std::io::BufWriter<std::fs::File>) -> std::io::Result<()>) -> std::io::Result<()> {
    use std::io::Write;
    // [sweep-sim] For the crash log.
    crate::crash::note_io("saving", path);
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.{}-{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let result = (|| {
        let mut out = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&tmp)?);
        fill(&mut out)?;
        out.flush()?;
        let f = out.into_inner().map_err(|e| e.into_error())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Sim::new())
            .configure_sets(Update, (SimSet::Input, SimSet::Step, SimSet::Sync).chain())
            .add_systems(Update, (sim_hotkeys.in_set(SimSet::Input), step_sim.in_set(SimSet::Step)));
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
    // [r5-drive] At the wheel Space is the handbrake.
    if keys.just_pressed(KeyCode::Space) && *mode != CameraMode::Drive {
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
    let began = std::time::Instant::now();
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
    sim.tick_ms = began.elapsed().as_secs_f32() * 1000.0;
    // Ticks delivered against ticks asked for, as a half-second moving
    // average: single frames carry 0 or 1 tick at low speeds, so the raw
    // ratio is meaningless without smoothing.
    let dt = time.delta_secs();
    if !sim.paused && dt > 0.0 {
        let ratio = ticks as f32 / (dt * sim.speed());
        // Averaged over a few ticks' worth of time: at 1x a tick comes only
        // every few seconds, and a half-second window read as 0%.
        let tau = (3.0 / sim.speed()).max(0.5);
        let k = 1.0 - (-dt / tau).exp();
        sim.effective_speed += (ratio - sim.effective_speed) * k;
    }
    let events = sim.world.drain_events();
    sim.events = events;
}
