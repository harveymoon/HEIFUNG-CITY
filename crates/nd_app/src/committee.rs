//! [committee] The committee in the running game: the backend chosen in the
//! settings (with the offline stand-in behind it), the daily alerts, the
//! calendar of meetings, and the meeting driver, pumped once a frame. The
//! sim is paused while a meeting sits and nothing else waits on a model:
//! requests go to a worker thread and come back on a later frame. The
//! window is `ui/committee.rs`, the remote API `remote/committee.rs`.

use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use std::time::Duration;

use bevy::prelude::*;
use nd_minds::committee::driver::Driver;
use nd_minds::committee::http::{backend_from, spawn_test};
use nd_minds::committee::{start_meeting, CommitteeSettings, Courier};

use crate::sim::{Sim, SimSet};
use crate::ui::UiFlags;

#[derive(Resource)]
pub struct CommitteeRt {
    pub settings: CommitteeSettings,
    pub courier: Courier,
    pub driver: Driver,
    /// Why the chosen backend is not in use (the stand-in speaks instead).
    pub backend_error: Option<String>,
    /// A "Test connection" in flight, and the last result.
    pub test: Option<Mutex<Receiver<Result<String, String>>>>,
    pub test_result: Option<Result<String, String>>,
    /// The sim's pause state before the meeting paused it.
    paused_before: Option<bool>,
    generation: u64,
    last_day: Option<u64>,
    /// Ask the window to open (the remote API, a meeting starting).
    pub open_request: Option<bool>,
    pub tab_request: Option<crate::ui::committee::Tab>,
    /// Transcript lines shown so far and characters of the next one: the
    /// window types the meeting out; Skip shows it all.
    pub shown: (usize, usize),
    pub fast_forward: bool,
    /// [voices] Model lines for spotlit and favourite citizens'
    /// conversations, and diaries on demand (display only).
    pub voices: nd_minds::committee::voices::Voices,
}

impl Default for CommitteeRt {
    fn default() -> Self {
        let settings = CommitteeSettings::load();
        let mut rt = CommitteeRt {
            courier: Courier::mock(),
            settings,
            driver: Driver::default(),
            backend_error: None,
            test: None,
            test_result: None,
            paused_before: None,
            generation: u64::MAX,
            last_day: None,
            open_request: None,
            tab_request: None,
            shown: (0, 0),
            fast_forward: false,
            voices: Default::default(),
        };
        rt.apply_settings();
        rt
    }
}

impl CommitteeRt {
    /// Build the backend the settings ask for; on an error the stand-in
    /// speaks and the error is shown.
    pub fn apply_settings(&mut self) {
        let timeout = Duration::from_secs(self.settings.timeout_s.max(5) + 5);
        match backend_from(&self.settings) {
            Ok(b) => {
                self.courier.set_backend(b);
                self.courier.timeout = timeout;
                self.backend_error = None;
            }
            Err(e) => {
                self.courier.set_backend(Box::new(nd_minds::committee::MockCommittee::default()));
                self.backend_error = Some(e);
            }
        }
    }

    pub fn save_settings(&mut self) -> Result<(), String> {
        self.settings.save()
    }

    pub fn test_connection(&mut self) {
        self.test = Some(Mutex::new(spawn_test(&self.settings)));
        self.test_result = None;
    }

    /// Call a meeting now (the window's button, the remote API).
    pub fn call_meeting(&mut self, sim: &mut Sim) -> Result<u32, String> {
        let n = start_meeting(&mut sim.world, true)?;
        self.begin(sim);
        Ok(n)
    }

    fn begin(&mut self, sim: &mut Sim) {
        if self.paused_before.is_none() {
            self.paused_before = Some(sim.paused);
        }
        sim.paused = true;
        self.shown = (0, 0);
        self.fast_forward = false;
        self.open_request = Some(true);
    }
}

pub struct CommitteePlugin;

impl Plugin for CommitteePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommitteeRt>()
            .add_systems(Update, committee_tick.in_set(SimSet::Sync))
            .add_systems(bevy_egui::EguiPrimaryContextPass, (crate::ui::committee::window_system, crate::ui::diary::window_system).after(crate::ui::side_panel));
    }
}

/// A new day: the committee's samples and alerts, and the meeting when one
/// is due (it pauses the city). Called once a frame, and by the remote
/// `step` after each chunk of ticks so a step of many days runs every day's
/// round, not one. Returns true when a meeting started.
pub fn daily(sim: &mut Sim, rt: &mut CommitteeRt, flags: &mut UiFlags) -> bool {
    let day = sim.world.clock.day();
    if rt.last_day == Some(day) {
        return false;
    }
    let first = rt.last_day.is_none();
    rt.last_day = Some(day);
    // Not on the frame a city appears: only when a day has passed.
    if first || sim.world.buildings.is_empty() {
        return false;
    }
    let ids = sim.world.committee_daily();
    for id in &ids {
        rt.driver.phrase_alert(&sim.world, *id, &mut rt.courier);
    }
    if let Some(a) = ids.last().and_then(|id| sim.world.committee.alerts.iter().find(|a| a.id == *id)) {
        let who = sim.world.committee.member(a.member).map_or_else(String::new, |p| format!("{} ({})", p.name, p.scope.label()));
        flags.message = format!("Committee alert from {who}: {}", a.text);
    }
    if sim.world.committee.meeting_due(day as u32) && start_meeting(&mut sim.world, false).is_ok() {
        rt.begin(sim);
        flags.message = "The committee meets: the city is paused for the meeting".into();
        return true;
    }
    false
}

/// Once a frame: alerts on a new day, the calendar, the meeting.
fn committee_tick(mut sim: ResMut<Sim>, mut rt: ResMut<CommitteeRt>, mut flags: ResMut<UiFlags>, tools: Res<crate::tools::ToolState>) {
    let rt = &mut *rt;
    if rt.generation != sim.generation {
        // A new city or a load: drop what was in flight; a meeting saved in
        // session resumes.
        rt.generation = sim.generation;
        rt.driver.reset();
        rt.voices.reset();
        rt.last_day = None;
        rt.paused_before = None;
        rt.shown = (0, 0);
        if sim.world.committee.meeting.is_some() {
            rt.paused_before = Some(false);
        }
    }
    if let Some(rx) = &rt.test {
        if let Some(r) = rx.lock().ok().and_then(|rx| rx.try_recv().ok()) {
            rt.test_result = Some(r);
            rt.test = None;
        }
    }
    daily(&mut sim, rt, &mut flags);
    // Replies: the meeting's and the alerts'.
    for d in rt.courier.poll() {
        // Neither: an answer to something forgotten (a reload).
        if !rt.driver.take(&mut sim.world, &d) {
            rt.voices.take(&d);
        }
        // An alert phrased just now: refresh the toast if it is the latest.
        if matches!(d.req.task, nd_minds::committee::Task::Alert { .. }) {
            if let Some(a) = sim.world.committee.alerts.last() {
                if a.phrased && flags.message.starts_with("Committee alert") {
                    let who = sim.world.committee.member(a.member).map_or_else(String::new, |p| format!("{} ({})", p.name, p.scope.label()));
                    flags.message = format!("Committee alert from {who}: {}", a.text);
                }
            }
        }
    }
    // [voices] New conversations of the people the player watches.
    {
        let w = &sim.world;
        let watched = |c: nd_sim::CitizenId| tools.favorites.contains(&c) || w.citizens.get(c).is_some_and(|x| x.spotlight);
        rt.voices.pump(w, &mut rt.courier, &watched);
    }
    if sim.world.committee.meeting.is_some() {
        if rt.paused_before.is_none() {
            rt.paused_before = Some(sim.paused);
        }
        sim.paused = true;
        rt.driver.advance(&mut sim.world, &mut rt.courier);
    } else if let Some(before) = rt.paused_before.take() {
        sim.paused = before;
        let n = sim.world.committee.minutes.last().map_or(0, |m| m.proposals.iter().filter(|p| p.project.is_some()).count());
        flags.message = match n {
            0 => "The committee adjourned".to_string(),
            1 => "The committee adjourned: one project waits for your review".to_string(),
            n => format!("The committee adjourned: {n} projects wait for your review"),
        };
    }
}
