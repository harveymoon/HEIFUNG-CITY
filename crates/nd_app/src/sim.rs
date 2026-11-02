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
/// [r7-sim-thread] Frames in a row with a write to the city after which the
/// ticks run in the frame (see `Sim::edit_streak`).
const EDIT_STREAK: u32 = 4;
/// [r7-sim-thread] The most ticks the sim thread runs in one batch.
const MAX_BATCH: u32 = 400;
/// Real time the sim may consume per frame before it yields to rendering.
const TICK_BUDGET: std::time::Duration = std::time::Duration::from_millis(12);

#[derive(Resource)]
pub struct Sim {
    /// [r7-sim-thread] The city as the screen has it: reads go straight to
    /// it (`Deref<Target = World>`); a write (`DerefMut`) makes it the city
    /// and sends the sim thread back to it (see `sim_thread`).
    pub world: crate::sim_thread::SimCore,
    /// The minds as they stand at `world`'s tick.
    pub minds: HybridMind,
    pub speed_index: usize,
    pub paused: bool,
    accumulator: f32,
    /// [r7-sim-thread] Ticks in the last batch shown: one tick's motion is
    /// spread over their time (`frac`).
    batch_ticks: u32,
    /// [r7-sim-thread] Pause when the clock reaches this tick (the remote
    /// `run_to`): the thread stops its batches there exactly.
    pub pause_at: Option<u64>,
    /// [r5-drive] [r7-sim-thread] The player's own car for the traffic,
    /// handed to every batch (Drive mode sets it every frame).
    pub player_car: Option<nd_sim::PlayerCar>,
    /// [r7-sim-thread] Tick on the sim thread (`ND_SIM_THREAD=0`: in the
    /// frame, as before).
    pub threaded: bool,
    /// [r7-sim-thread] Frames in a row in which something wrote the city
    /// between two `advance`s, and the write count the last one ended at.
    /// A window that takes `&mut` to the world every frame (open, not
    /// clicked) would make the thread start over every frame and the city
    /// stand still: after [`EDIT_STREAK`] such frames the ticks run in the
    /// frame, as before the thread, until the writes stop.
    pub edit_streak: u32,
    edits_seen: u64,
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
    /// [K34] The real boxes on screen (depots and trains), for picking:
    /// record, floor centre, rotation, forty-footer.
    pub box_spots: Vec<(nd_sim::containers::BoxId, bevy::math::Vec3, bevy::math::Quat, bool)>,
}

impl Sim {
    pub fn new() -> Self {
        let world = Self::fresh_world(nd_sim::LandscapeParams::default());
        let (start, start_gestures) = (world.clock.tick, world.master.gestures.len());
        Self {
            world: crate::sim_thread::SimCore::new(world),
            minds: HybridMind::new(Box::new(MockBackend::default())),
            speed_index: DEFAULT_SPEED,
            paused: false,
            accumulator: 0.0,
            batch_ticks: 1,
            pause_at: None,
            player_car: None,
            threaded: std::env::var("ND_SIM_THREAD").map_or(true, |v| v != "0"),
            edit_streak: 0,
            edits_seen: 0,
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
        self.world.replace(Self::fresh_world(landscape));
        // Answers still out are about the old city's citizens.
        nd_sim::MindProvider::reset(&mut self.minds);
        self.events.clear();
        self.accumulator = 0.0;
        self.batch_ticks = 1;
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
    /// [r7-sim-thread] A batch of several ticks (the top speeds) shows its
    /// last tick's motion over the whole batch's time.
    pub fn frac(&self) -> f32 {
        if self.paused {
            1.0
        } else {
            let n = self.batch_ticks.max(1) as f32;
            ((self.accumulator + n - 1.0) / n).clamp(0.0, 1.0)
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
        if n > 0 && (self.player_car.is_some() || self.world.traffic.player.is_some()) {
            self.world.traffic.player = self.player_car.clone();
        }
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
        self.world.replace(World::read_save(path)?);
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
        self.pause_at = None;
        self.player_car = None;
    }

    /// [r7-sim-thread] Move the game on by `dt` real seconds: what the sim
    /// thread finished is shown when the clock reaches it and the next
    /// batch asked for; edits made since the last frame go to the thread
    /// first. Returns the ticks shown this frame.
    pub fn advance(&mut self, dt: f32) -> u32 {
        let began = std::time::Instant::now();
        let edits = self.world.stats_edits();
        self.edit_streak = if edits != self.edits_seen { self.edit_streak + 1 } else { 0 };
        let ticks = if self.threaded && self.edit_streak < EDIT_STREAK { self.advance_threaded(dt) } else { None };
        let ticks = ticks.unwrap_or_else(|| self.advance_in_frame(dt));
        self.edits_seen = self.world.stats_edits();
        self.ticks_last_frame = ticks;
        self.tick_ms = began.elapsed().as_secs_f32() * 1000.0;
        // Ticks delivered against ticks asked for, as a half-second moving
        // average: single frames carry 0 or 1 tick at low speeds, so the raw
        // ratio is meaningless without smoothing.
        if !self.paused && dt > 0.0 {
            let ratio = ticks as f32 / (dt * self.speed());
            // Averaged over a few ticks' worth of time: at 1x a tick comes only
            // every few seconds, and a half-second window read as 0%.
            let tau = (3.0 / self.speed()).max(0.5);
            let k = 1.0 - (-dt / tau).exp();
            self.effective_speed += (ratio - self.effective_speed) * k;
        }
        ticks
    }

    /// The old way, and the fallback: tick here, in the frame, within
    /// [`TICK_BUDGET`].
    fn advance_in_frame(&mut self, dt: f32) -> u32 {
        let mut ticks = 0;
        if !self.paused {
            self.accumulator += dt * self.speed();
            self.batch_ticks = 1;
            let start = std::time::Instant::now();
            if self.player_car.is_some() || self.world.traffic.player.is_some() {
                self.world.traffic.player = self.player_car.clone();
            }
            while self.accumulator >= 1.0 && ticks < MAX_TICKS_PER_FRAME && !self.at_pause_tick() {
                self.world.tick(&mut self.minds);
                self.accumulator -= 1.0;
                ticks += 1;
                if start.elapsed() > TICK_BUDGET {
                    break;
                }
            }
            // Drop any backlog we could not afford: the sim runs slower, the frame stays smooth.
            if self.accumulator > 2.0 {
                self.accumulator = 1.0;
            }
            self.check_pause_at();
        }
        let mut events = std::mem::take(&mut self.events);
        events.clear();
        events.extend(self.world.take_events());
        self.events = events;
        ticks
    }

    /// The clock is at `pause_at`.
    fn at_pause_tick(&self) -> bool {
        self.pause_at.is_some_and(|t| self.world.clock.tick >= t)
    }

    fn check_pause_at(&mut self) {
        if self.at_pause_tick() {
            self.paused = true;
            self.pause_at = None;
            self.accumulator = 0.0;
        }
    }

    /// Ticks to ask the thread for: enough that the batch covers the time it
    /// takes (its copy and its ticks) at this speed, with a quarter to
    /// spare; as many as fit in a batch when the thread cannot keep up.
    fn batch_target(&self) -> u32 {
        let s = &self.world.stats;
        let (copy, tick) = (s.copy_ms.max(1.0), s.tick_ms.max(0.5));
        let per_s = self.speed();
        let room = 1000.0 - 1.25 * tick * per_s;
        let n = if room < 50.0 { MAX_BATCH as f32 } else { (1.25 * copy * per_s / room).ceil() };
        (n as u32).clamp(1, MAX_BATCH)
    }

    /// The sim thread's frame (see `sim_thread`). None when the thread
    /// cannot run: tick in the frame.
    fn advance_threaded(&mut self, dt: f32) -> Option<u32> {
        self.world.poll();
        // The screen's own edits since the last frame, before anything is
        // handed to the thread (a copy of the world carries its events).
        let mut events = std::mem::take(&mut self.events);
        events.clear();
        events.extend(self.world.take_events());
        let mut ticks = 0;
        if !self.paused {
            self.check_pause_at();
        }
        if !self.paused {
            self.accumulator += dt * self.speed();
            if self.accumulator >= 1.0 {
                if let Some(b) = self.world.adopt() {
                    ticks = b.ticks;
                    self.minds = b.minds;
                    events.extend(b.events);
                    self.accumulator -= ticks as f32;
                    self.batch_ticks = ticks.max(1);
                    self.check_pause_at();
                }
            }
            // A backlog the thread cannot catch up: show batches as they
            // come (the sim runs slower, the frame stays smooth).
            if self.accumulator > (self.batch_target() + 2) as f32 {
                self.accumulator = 1.0;
            }
        }
        self.events = events;
        // [r5-drive] At the wheel the traffic must see the car where it is
        // now: no batch ahead, the tick is asked for when it is due.
        let want = if self.paused || (self.player_car.is_some() && self.accumulator < 1.0) {
            0
        } else {
            let n = self.batch_target();
            match self.pause_at {
                Some(t) => n.min(t.saturating_sub(self.world.clock.tick).min(MAX_BATCH as u64) as u32),
                None => n,
            }
        };
        // Flat out (the thread cannot keep up): longer batches, fewer copies.
        let budget = if self.batch_target() >= MAX_BATCH {
            let copy = std::time::Duration::from_secs_f32(self.world.stats.copy_ms.max(0.0) * 2.0 / 1000.0);
            copy.clamp(crate::sim_thread::BATCH_BUDGET, crate::sim_thread::BATCH_BUDGET_MAX)
        } else {
            crate::sim_thread::BATCH_BUDGET
        };
        if !self.world.run(&self.minds, want, budget, self.player_car.clone()) {
            self.threaded = false;
            return None;
        }
        Some(ticks)
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
    sim.advance(time.delta_secs());
}
