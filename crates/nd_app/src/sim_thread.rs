//! [r7-sim-thread] The simulation on a thread of its own.
//!
//! The screen used to tick the world itself, in the frame: every tick
//! (30 ms at 15,000 people, 90 at 38,000, up to 0.8 s at a shift start)
//! was a frame the player saw stall. Now a sim thread owns the city that
//! ticks and hands the screen a finished copy after every batch of ticks.
//!
//! How the two copies stay one city:
//! - The sim thread keeps its own world (`Worker::state`) and ticks it in
//!   batches when the screen asks (`Run`). After a batch it copies the world
//!   and sends the copy (`Batch`): the screen shows it and keeps it as
//!   [`SimCore`], which everything in the app reads through `Deref` exactly
//!   as it read `Sim::world` before.
//! - The screen's copy is also the one the player edits: a tool, a button or
//!   a remote command takes `&mut` through `DerefMut`, which is the only way
//!   to get one. An edit makes the screen's copy the city (`dirty`): ticks
//!   the thread ran ahead from the old copy are thrown away (they were never
//!   shown), and the thread starts again from a copy of the edited world and
//!   the minds as they were (`Rebase`). So an edit lands on the tick the
//!   player saw, between two ticks, as it always did, and the same edits on
//!   the same ticks give the same city whichever thread ticked it.
//! - The thread runs ahead by one batch at most: the batch waits in `ready`
//!   until the screen's clock reaches it, so at low speeds the next tick is
//!   there when it is due and nothing waits for it.
//! - Copies the screen lets go (the city a batch replaced, a stale batch)
//!   are dropped on a third thread: freeing a big city takes tens of ms.
//!
//! `ND_SIM_THREAD=0` ticks in the frame as before (see `Sim::advance`).

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use nd_minds::HybridMind;
use nd_sim::{PlayerCar, SimEvent, World};

/// [r7-sim-thread] What the threads behind the screen are doing (bits:
/// 1 ticking, 2 copying the world for the screen or a rebase, 4 freeing a
/// world), and what they did since the screen last looked (`frame_stats`
/// lists it beside each slow frame).
pub static PHASE: AtomicU32 = AtomicU32::new(0);
pub static PHASE_SEEN: AtomicU32 = AtomicU32::new(0);

fn phase_on(bit: u32) {
    PHASE.fetch_or(bit, Ordering::Relaxed);
    PHASE_SEEN.fetch_or(bit, Ordering::Relaxed);
}

fn phase_off(bit: u32) {
    PHASE.fetch_and(!bit, Ordering::Relaxed);
}

/// The phases seen since the last call (and those still on).
pub fn phases_seen() -> u32 {
    PHASE_SEEN.swap(PHASE.load(Ordering::Relaxed), Ordering::Relaxed)
}

/// The longest a batch ticks before the thread hands it over: at the top
/// speeds the screen still gets a fresh city a few times a second. When the
/// thread cannot keep up with the speed asked for, batches run longer (two
/// copies' time, at most [`BATCH_BUDGET_MAX`]) so the copies cost at most a
/// third of its time (`Sim::advance`): more days a minute, a fresh city
/// shown a little less often.
pub const BATCH_BUDGET: Duration = Duration::from_millis(200);
pub const BATCH_BUDGET_MAX: Duration = Duration::from_millis(600);

enum ToWorker {
    /// Start again from this world and these minds (an edit, a load).
    Rebase { base: Arc<World>, minds: HybridMind, epoch: u64 },
    /// Tick up to `max_ticks` (stopping at the end of a game hour or after
    /// its budget) and send a copy.
    Run { epoch: u64, max_ticks: u32, budget: Duration, player: Option<PlayerCar> },
}

enum FromWorker {
    /// The thread has its own copy of the rebased world (the screen's
    /// copy is the screen's alone again).
    Rebased { epoch: u64 },
    Batch(Batch),
    /// The run stopped because the screen's city changed under it.
    Aborted { epoch: u64 },
}

/// A batch of ticks, finished.
pub struct Batch {
    epoch: u64,
    world: Arc<World>,
    minds: HybridMind,
    ticks: u32,
    events: Vec<SimEvent>,
    /// Ms ticking, the slowest tick, and the copy for the screen.
    tick_ms: f32,
    max_tick_ms: f32,
    copy_ms: f32,
}

/// A batch taken in by the screen ([`SimCore::adopt`]).
pub struct Adopted {
    pub minds: HybridMind,
    pub ticks: u32,
    pub events: Vec<SimEvent>,
}

/// What the sim thread has been doing, for the status, `frame_stats` and
/// the batch sizes.
#[derive(Clone, Debug, Default)]
pub struct ThreadStats {
    /// Smoothed ms a tick and ms a copy, on the sim thread.
    pub tick_ms: f32,
    pub copy_ms: f32,
    pub max_tick_ms: f32,
    pub batches: u64,
    pub ticks: u64,
    /// Batches (or runs) thrown away because the player edited first.
    pub discarded: u64,
    pub rebases: u64,
    /// Ms the screen waited for the thread (an edit while the thread still
    /// copied the last edit), in total.
    pub waited_ms: f32,
}

struct Link {
    tx: mpsc::Sender<ToWorker>,
    // A Mutex only so `Sim` is `Sync` (a Bevy resource); only the main
    // thread reads it, through `get_mut`.
    rx: Mutex<mpsc::Receiver<FromWorker>>,
    /// The screen's epoch: the thread stops a run whose epoch went stale.
    current: Arc<AtomicU64>,
    dropper: mpsc::Sender<Garbage>,
}

/// A city to free off the main thread (held only to be dropped there).
#[allow(dead_code)]
enum Garbage {
    Shared(Arc<World>),
    Own(Box<World>),
}

/// The city as the screen has it, and the line to the sim thread.
pub struct SimCore {
    world: Arc<World>,
    link: Option<Link>,
    /// Bumped by every edit and every replacement of the city: a batch
    /// from an older epoch grew from a city that is no longer this one.
    epoch: u64,
    /// The screen's copy changed since the thread last got it.
    dirty: bool,
    /// A rebase is out: the thread is still copying the screen's world, so
    /// the screen must wait before it edits it again.
    rebase_out: Option<u64>,
    /// The epoch of the run out, if any.
    run_out: Option<u64>,
    ready: Option<Batch>,
    pub stats: ThreadStats,
    /// Edits by where in the code they came from (`#[track_caller]`).
    edits: HashMap<(&'static str, u32), u64>,
    log_edits: bool,
}

impl SimCore {
    pub fn new(world: World) -> Self {
        Self {
            world: Arc::new(world),
            link: None,
            epoch: 1,
            dirty: true,
            rebase_out: None,
            run_out: None,
            ready: None,
            stats: ThreadStats::default(),
            edits: HashMap::new(),
            log_edits: std::env::var_os("ND_EDIT_LOG").is_some_and(|v| v != "0"),
        }
    }

    /// The city as it stands, shared (no copy): a save written on another
    /// thread while the game goes on. An edit made while it is held copies
    /// the world first.
    pub fn share(&self) -> Arc<World> {
        self.world.clone()
    }

    /// Replace the city (a load, a new game): whatever the thread was
    /// doing is thrown away.
    #[track_caller]
    pub fn replace(&mut self, world: World) {
        self.touch(Location::caller());
        let old = std::mem::replace(&mut self.world, Arc::new(world));
        self.drop_later(Garbage::Shared(old));
    }

    /// A copy of the world itself (tests).
    pub fn to_world(&self) -> World {
        (*self.world).clone()
    }

    fn drop_later(&self, g: Garbage) {
        if let Some(link) = &self.link {
            if let Err(mpsc::SendError(g)) = link.dropper.send(g) {
                drop(g);
            }
        }
    }

    /// The player (or the API) changes the city: it is the screen's copy
    /// from here on, and whatever the thread ran ahead is void.
    fn touch(&mut self, at: &'static Location<'static>) {
        *self.edits.entry((at.file(), at.line())).or_insert(0) += 1;
        if !self.dirty {
            self.dirty = true;
            self.epoch += 1;
            if let Some(link) = &self.link {
                link.current.store(self.epoch, Ordering::Release);
            }
            let thrown = self.ready.is_some() || self.run_out.is_some();
            if thrown {
                self.stats.discarded += 1;
            }
            if self.log_edits {
                bevy::log::info!("[sim-thread] edit at {}:{}{}", at.file(), at.line(), if thrown { " (threw away the batch ahead)" } else { "" });
            }
        }
        if let Some(b) = self.ready.take() {
            self.drop_later(Garbage::Shared(b.world));
        }
    }

    /// Make the screen's copy its own before it is written: wait for the
    /// thread to finish copying the last rebase; anyone else holding it (a
    /// save being written) gets the old one and the screen a copy.
    fn unshare(&mut self) {
        if Arc::get_mut(&mut self.world).is_some() {
            return;
        }
        if self.rebase_out.is_some() && self.link.is_some() {
            let t = Instant::now();
            while self.rebase_out.is_some() {
                let Some(msg) = self.link.as_mut().and_then(|l| l.rx.get_mut().ok()?.recv().ok()) else {
                    self.rebase_out = None;
                    break;
                };
                self.receive(msg);
            }
            self.stats.waited_ms += t.elapsed().as_secs_f32() * 1000.0;
        }
        // A save still holds it: this copy is the hitch the old autosave
        // paid every day, now only when an edit meets a save in progress.
        Arc::make_mut(&mut self.world);
    }

    fn receive(&mut self, msg: FromWorker) {
        match msg {
            FromWorker::Rebased { epoch } => {
                if self.rebase_out == Some(epoch) {
                    self.rebase_out = None;
                }
            }
            FromWorker::Aborted { epoch } => {
                if self.run_out == Some(epoch) {
                    self.run_out = None;
                }
            }
            FromWorker::Batch(b) => {
                if self.run_out == Some(b.epoch) {
                    self.run_out = None;
                }
                let s = &mut self.stats;
                s.batches += 1;
                s.ticks += b.ticks as u64;
                if b.ticks > 0 {
                    let per = b.tick_ms / b.ticks as f32;
                    s.tick_ms = if s.tick_ms == 0.0 { per } else { s.tick_ms + (per - s.tick_ms) * 0.2 };
                }
                s.copy_ms = if s.copy_ms == 0.0 { b.copy_ms } else { s.copy_ms + (b.copy_ms - s.copy_ms) * 0.2 };
                s.max_tick_ms = s.max_tick_ms.max(b.max_tick_ms);
                if b.epoch == self.epoch && !self.dirty && self.ready.is_none() {
                    self.ready = Some(b);
                } else {
                    self.drop_later(Garbage::Shared(b.world));
                }
            }
        }
    }

    /// Take in what the thread sent, without waiting.
    pub fn poll(&mut self) {
        loop {
            let Some(msg) = self.link.as_mut().and_then(|l| l.rx.get_mut().ok()?.try_recv().ok()) else { break };
            self.receive(msg);
        }
    }

    /// A batch is waiting to be shown.
    pub fn has_ready(&self) -> bool {
        self.ready.is_some()
    }

    /// Show the batch waiting: it becomes the screen's city. Returns what
    /// came with it: the minds, the events and the tick count.
    pub fn adopt(&mut self) -> Option<Adopted> {
        let b = self.ready.take()?;
        let old = std::mem::replace(&mut self.world, b.world);
        self.drop_later(Garbage::Shared(old));
        Some(Adopted { minds: b.minds, ticks: b.ticks, events: b.events })
    }

    /// The events the screen's own edits raised (a road laid, a building
    /// placed), for the renderers this frame.
    pub fn take_events(&mut self) -> Vec<SimEvent> {
        if self.world.events.is_empty() {
            return Vec::new();
        }
        // Not an edit: the thread never sees these events, they were
        // raised on the screen's copy.
        self.unshare();
        Arc::get_mut(&mut self.world).map(|w| w.drain_events()).unwrap_or_default()
    }

    /// The thread is running (it starts on the first [`Self::run`]).
    pub fn threaded(&self) -> bool {
        self.link.is_some()
    }

    fn start(&mut self) {
        if self.link.is_some() {
            return;
        }
        let (tx, worker_rx) = mpsc::channel();
        let (worker_tx, rx) = mpsc::channel();
        let (dropper, garbage) = mpsc::channel::<Garbage>();
        let current = Arc::new(AtomicU64::new(self.epoch));
        let cur = current.clone();
        let drop_tx = dropper.clone();
        let spawned = std::thread::Builder::new()
            .name("nd-sim".into())
            .stack_size(256 << 20)
            .spawn(move || Worker { rx: worker_rx, tx: worker_tx, current: cur, dropper: drop_tx, state: None }.run());
        if spawned.is_err() {
            return;
        }
        let _ = std::thread::Builder::new().name("nd-sim-free".into()).spawn(move || {
            for g in garbage {
                phase_on(4);
                drop(g);
                phase_off(4);
            }
        });
        self.link = Some(Link { tx, rx: Mutex::new(rx), current, dropper });
        self.dirty = true;
    }

    /// Hand the thread the screen's city if it changed, then, when no batch
    /// is out or waiting, ask for the next one. `minds` is the screen's
    /// copy of the minds (the state at this city). Returns false when the
    /// thread cannot run (it could not start, or the minds cannot be
    /// copied): tick in the frame then.
    pub fn run(&mut self, minds: &HybridMind, max_ticks: u32, budget: Duration, player: Option<PlayerCar>) -> bool {
        self.start();
        if self.link.is_none() {
            return false;
        }
        if self.dirty {
            let Some(m) = minds.try_clone() else { return false };
            self.stats.rebases += 1;
            let base = Arc::clone(&self.world);
            let epoch = self.epoch;
            let link = self.link.as_ref().unwrap();
            link.current.store(epoch, Ordering::Release);
            if link.tx.send(ToWorker::Rebase { base, minds: m, epoch }).is_err() {
                self.link = None;
                return false;
            }
            self.rebase_out = Some(epoch);
            self.dirty = false;
        }
        if max_ticks == 0 || self.ready.is_some() || self.run_out == Some(self.epoch) {
            return true;
        }
        let epoch = self.epoch;
        if self.link.as_ref().unwrap().tx.send(ToWorker::Run { epoch, max_ticks, budget, player }).is_err() {
            self.link = None;
            return false;
        }
        self.run_out = Some(epoch);
        true
    }

    /// A batch is out on the thread.
    pub fn running(&self) -> bool {
        self.run_out == Some(self.epoch)
    }

    /// Wait for the batch out, if any (tests, and the remote API's
    /// `wait`): it is then [`Self::has_ready`].
    pub fn wait_batch(&mut self, timeout: Duration) -> bool {
        let t = Instant::now();
        while self.running() && self.ready.is_none() {
            let left = timeout.saturating_sub(t.elapsed());
            if left.is_zero() {
                return false;
            }
            let Some(msg) = self.link.as_mut().and_then(|l| l.rx.get_mut().ok()?.recv_timeout(left).ok()) else { return false };
            self.receive(msg);
        }
        self.ready.is_some()
    }

    /// Writes to the city so far.
    pub fn stats_edits(&self) -> u64 {
        self.edits.values().sum()
    }

    /// Edits so far by where they came from, most first.
    pub fn edit_sites(&self, n: usize) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = self.edits.iter().map(|((f, l), c)| (format!("{f}:{l}"), *c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.truncate(n);
        v
    }
}

impl Drop for SimCore {
    fn drop(&mut self) {
        // Closing the channel ends the thread after its current message.
        self.link = None;
    }
}

impl Deref for SimCore {
    type Target = World;
    fn deref(&self) -> &World {
        &self.world
    }
}

impl DerefMut for SimCore {
    /// Every write to the city comes through here: it makes the screen's
    /// copy the city (see the module notes).
    #[track_caller]
    fn deref_mut(&mut self) -> &mut World {
        self.touch(Location::caller());
        self.unshare();
        Arc::get_mut(&mut self.world).expect("unshared")
    }
}

struct Worker {
    rx: mpsc::Receiver<ToWorker>,
    tx: mpsc::Sender<FromWorker>,
    current: Arc<AtomicU64>,
    dropper: mpsc::Sender<Garbage>,
    state: Option<(Box<World>, HybridMind, u64)>,
}

impl Worker {
    fn run(mut self) {
        while let Ok(msg) = self.rx.recv() {
            match msg {
                ToWorker::Rebase { base, minds, epoch } => {
                    if let Some((old, _, _)) = self.state.take() {
                        let _ = self.dropper.send(Garbage::Own(old));
                    }
                    phase_on(2);
                    let w = Box::new(nd_sim::versioned::exact_clone(|| (*base).clone()));
                    phase_off(2);
                    drop(base);
                    self.state = Some((w, minds, epoch));
                    if self.tx.send(FromWorker::Rebased { epoch }).is_err() {
                        return;
                    }
                }
                ToWorker::Run { epoch, max_ticks, budget, player } => {
                    let reply = self.batch(epoch, max_ticks, budget, player);
                    if self.tx.send(reply).is_err() {
                        return;
                    }
                }
            }
        }
    }

    fn batch(&mut self, epoch: u64, max_ticks: u32, budget: Duration, player: Option<PlayerCar>) -> FromWorker {
        let current = &self.current;
        let stale = || current.load(Ordering::Acquire) != epoch;
        let Some((w, minds, e)) = self.state.as_mut() else { return FromWorker::Aborted { epoch } };
        if *e != epoch || stale() {
            return FromWorker::Aborted { epoch };
        }
        // [r5-drive] The player's car, as the screen last had it.
        w.traffic.player = player;
        let t0 = Instant::now();
        let (mut ticks, mut max_ms) = (0u32, 0f32);
        phase_on(1);
        while ticks < max_ticks {
            if stale() {
                break;
            }
            let t = Instant::now();
            w.tick(minds);
            max_ms = max_ms.max(t.elapsed().as_secs_f32() * 1000.0);
            ticks += 1;
            // The screen sees every game hour's last minute (the history
            // reads the day's counters there, as `step` does) and a city a
            // few times a second at the top speeds.
            if w.clock.tick % 60 == 59 || t0.elapsed() >= budget {
                break;
            }
        }
        phase_off(1);
        let tick_ms = t0.elapsed().as_secs_f32() * 1000.0;
        if stale() {
            // The screen's city changed: a rebase is on its way.
            return FromWorker::Aborted { epoch };
        }
        let events = w.drain_events();
        let c = Instant::now();
        phase_on(2);
        let copy = Arc::new(nd_sim::versioned::exact_clone(|| (**w).clone()));
        phase_off(2);
        let Some(minds) = minds.try_clone() else { return FromWorker::Aborted { epoch } };
        let copy_ms = c.elapsed().as_secs_f32() * 1000.0;
        FromWorker::Batch(Batch { epoch, world: copy, minds, ticks, events, tick_ms, max_tick_ms: max_ms, copy_ms })
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use std::time::Duration;

    /// A demo district an hour in, and a second Sim holding exactly the
    /// same city and minds.
    fn twins() -> (Sim, Sim) {
        let mut a = Sim::new();
        a.seed_demo_district();
        a.step_ticks(90);
        let _ = a.world.take_events();
        let mut b = Sim::new();
        b.world.replace(a.world.to_world());
        b.minds = a.minds.try_clone().expect("the mock mind copies");
        a.speed_index = crate::sim::SPEEDS.len() - 1;
        a.paused = false;
        (a, b)
    }

    fn fp(s: &Sim) -> u64 {
        s.world.fingerprint().expect("fingerprint").0
    }

    /// Frames until the threaded Sim pauses at its `pause_at`; the events
    /// it showed, by kind.
    fn run_threaded(a: &mut Sim) -> std::collections::BTreeMap<String, u32> {
        let mut seen = std::collections::BTreeMap::new();
        for _ in 0..100_000 {
            if a.paused {
                break;
            }
            a.advance(1.0);
            for e in &a.events {
                *seen.entry(format!("{e:?}").split(['(', ' ', '{']).next().unwrap().to_string()).or_insert(0) += 1;
            }
            a.world.wait_batch(Duration::from_secs(30));
        }
        assert!(a.paused, "run_to never arrived");
        seen
    }

    fn run_here(b: &mut Sim, ticks: u32) -> std::collections::BTreeMap<String, u32> {
        let mut seen = std::collections::BTreeMap::new();
        for _ in 0..ticks {
            b.step_ticks(1);
            for e in b.world.take_events() {
                *seen.entry(format!("{e:?}").split(['(', ' ', '{']).next().unwrap().to_string()).or_insert(0) += 1;
            }
        }
        seen
    }

    /// The thread's ticks are the frame's ticks: the same city, the same
    /// events shown once each, and `run_to` stops on its tick exactly.
    #[test]
    fn the_sim_thread_grows_the_same_city() {
        let (mut a, mut b) = twins();
        let start = a.world.clock.tick;
        a.pause_at = Some(start + 300);
        let seen_a = run_threaded(&mut a);
        assert!(a.world.threaded(), "ran on the thread");
        assert_eq!(a.world.clock.tick, start + 300, "paused on the tick asked for");
        assert!(a.world.stats.batches > 1);
        let seen_b = run_here(&mut b, 300);
        assert_eq!(seen_a, seen_b, "every event shown once");
        assert_eq!(fp(&a), fp(&b), "the same city");
    }

    /// An edit lands between the ticks the screen showed, whatever the
    /// thread had run ahead: the batch ahead is thrown away, and the city
    /// is the one the same edit on the same tick gives without a thread.
    #[test]
    fn an_edit_lands_on_the_tick_shown() {
        let (mut a, mut b) = twins();
        let start = a.world.clock.tick;
        a.pause_at = Some(start + 400);
        let mut edited_at = None;
        for _ in 0..100_000 {
            if a.paused {
                break;
            }
            a.advance(1.0);
            a.world.wait_batch(Duration::from_secs(30));
            // A batch is waiting ahead: edit now, on the tick shown.
            if edited_at.is_none() && a.world.clock.tick >= start + 120 && a.world.has_ready() {
                edited_at = Some(a.world.clock.tick);
                a.world.economy.treasury += 12_345.0;
                a.world.add_road(&[nd_sim::Vec2::new(-260.0, 330.0), nd_sim::Vec2::new(260.0, 330.0)], 1, 1);
                assert!(!a.world.has_ready(), "the batch ahead is void");
            }
        }
        let t = edited_at.expect("edited");
        assert!(a.world.stats.discarded >= 1);
        assert_eq!(a.world.clock.tick, start + 400);
        let _ = run_here(&mut b, (t - start) as u32);
        b.world.economy.treasury += 12_345.0;
        b.world.add_road(&[nd_sim::Vec2::new(-260.0, 330.0), nd_sim::Vec2::new(260.0, 330.0)], 1, 1);
        let _ = run_here(&mut b, (start + 400 - t) as u32);
        assert_eq!(fp(&a), fp(&b), "the edit landed on tick {t} in both");
    }

    /// Pause holds the city at once, with a batch ready or out; the speed
    /// sizes the batches.
    #[test]
    fn pause_stops_the_city_at_once() {
        let (mut a, _) = twins();
        a.advance(1.0);
        a.world.wait_batch(Duration::from_secs(30));
        a.advance(1.0);
        let shown = a.world.clock.tick;
        a.paused = true;
        for _ in 0..20 {
            a.advance(1.0);
            a.world.wait_batch(Duration::from_secs(30));
        }
        assert_eq!(a.world.clock.tick, shown, "nothing shown while paused");
        a.paused = false;
        a.advance(1.0);
        assert!(a.world.clock.tick > shown, "the batch waiting shows on resume");
    }

    /// Edits while paused reach the thread (a rebase), and a write every
    /// frame is counted by where it came from.
    #[test]
    fn edits_are_counted_by_site() {
        let (mut a, _) = twins();
        a.paused = true;
        for _ in 0..3 {
            a.world.economy.treasury += 1.0;
            a.advance(0.016);
        }
        let sites = a.world.edit_sites(4);
        assert!(sites.iter().any(|(s, n)| s.contains("sim_thread.rs") && *n >= 3), "{sites:?}");
        assert!(a.world.stats.rebases >= 1);
    }
}
