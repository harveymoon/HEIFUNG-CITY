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
    /// [fix/review-app] The player let the remote API change the backend
    /// and its connection (this run only). Off, a script cannot switch the
    /// committee to a paid model or point it at another server.
    pub remote_may_switch: bool,
    /// [fix/roads-tools] The project whose site Show outlines on the map,
    /// and when it was asked (app seconds); it fades after `SHOW_SITE_FOR`.
    pub shown_site: Option<(u32, f64)>,
}

/// [fix/roads-tools] How long a project's Show outlines its site, seconds.
pub const SHOW_SITE_FOR: f64 = 45.0;

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
            remote_may_switch: false,
            shown_site: None,
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
            .add_systems(bevy_egui::EguiPrimaryContextPass, (crate::ui::committee::window_system, crate::ui::diary::window_system).after(crate::ui::side_panel))
            .add_systems(Update, draw_shown_site.in_set(crate::overlay::OverlaySet::Draw));
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

/// [fix/roads-tools] What a committee project's site would change, for
/// Show: the parcels it zones, the lot its building would take (or the
/// ground under its spot), the buildings standing there now, the clean-up
/// circle and the bus stops. Empty for a project with no site.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SiteMarks {
    pub ground: Vec<Vec<nd_sim::Vec2>>,
    pub buildings: Vec<Vec<nd_sim::Vec2>>,
    pub circle: Option<(nd_sim::Vec2, f32)>,
    pub stops: Vec<nd_sim::Vec2>,
}

pub fn site_marks(w: &nd_sim::World, s: &nd_sim::committee::SiteOption) -> SiteMarks {
    let mut out = SiteMarks::default();
    let mut lots: Vec<nd_sim::LotId> = Vec::new();
    for k in &s.parcels {
        let Some(i) = w.parcels.find(k) else { continue };
        let parcel = &w.parcels.parcels[i];
        out.ground.push(parcel.polygon.clone());
        if let Some(l) = parcel.lot.filter(|l| w.lots.contains_key(*l)) {
            lots.push(l);
        }
    }
    if let Some(p) = s.point {
        if let Some((id, l)) = w.lots.iter().find(|(_, l)| l.contains(p)) {
            out.ground.push(l.polygon.clone());
            lots.push(id);
        } else if let Some(i) = w.parcels.at(p) {
            out.ground.push(w.parcels.parcels[i].polygon.clone());
        }
        if s.radius > 0.0 {
            out.circle = Some((p, s.radius));
        }
    }
    lots.sort();
    lots.dedup();
    for l in lots {
        if w.lots[l].building.is_some() {
            out.buildings.push(w.lots[l].polygon.clone());
        }
    }
    out.stops = s.stops.clone();
    out
}

fn draw_shown_site(mut gizmos: Gizmos, sim: Res<Sim>, rt: Res<CommitteeRt>, time: Res<Time>) {
    let Some((id, at)) = rt.shown_site else { return };
    let now = time.elapsed_secs_f64();
    if now - at > SHOW_SITE_FOR {
        return;
    }
    let w = &sim.world;
    let Some(site) = w.committee.project(id).and_then(|p| p.site()) else { return };
    let marks = site_marks(w, site);
    let lift = |q: nd_sim::Vec2, up: f32| Vec3::new(q.x, w.surface_height(q) + up, q.y);
    let pulse = 0.75 + 0.25 * (now as f32 * 4.0).sin();
    let magenta = Color::linear_rgb(3.0 * pulse, 0.4, 3.0 * pulse);
    let orange = Color::linear_rgb(3.2 * pulse, 1.2 * pulse, 0.2);
    for poly in &marks.ground {
        for up in [0.6, 1.2] {
            let mut pts: Vec<Vec3> = poly.iter().map(|&q| lift(q, up)).collect();
            if let Some(&f) = pts.first() {
                pts.push(f);
            }
            gizmos.linestrip(pts, magenta);
        }
    }
    for poly in &marks.buildings {
        for up in [2.0, 8.0, 14.0] {
            let mut pts: Vec<Vec3> = poly.iter().map(|&q| lift(q, up)).collect();
            if let Some(&f) = pts.first() {
                pts.push(f);
            }
            gizmos.linestrip(pts, orange);
        }
    }
    if let Some((c, r)) = marks.circle {
        gizmos.circle(Isometry3d::new(lift(c, 1.0), Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), r, magenta);
    }
    for &s in &marks.stops {
        gizmos.line(lift(s, 0.5), lift(s, 25.0), magenta);
        gizmos.circle(Isometry3d::new(lift(s, 1.0), Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)), 5.0, magenta);
    }
}

#[cfg(test)]
mod site_tests {
    use super::*;
    use nd_sim::Vec2;

    /// [fix/roads-tools] Show outlines the parcels a rezoning paints and,
    /// for a building's spot, the ground it would take.
    #[test]
    fn a_site_shows_its_parcels_and_the_ground_at_its_spot() {
        let mut w = nd_sim::World::new(nd_sim::WorldConfig { emergencies: false, society: false, landscape: nd_sim::LandscapeParams::flat(), ..Default::default() });
        w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        let keys: Vec<_> = w.parcels.parcels.iter().take(3).map(|p| p.key).collect();
        assert_eq!(keys.len(), 3);
        let site = nd_sim::committee::SiteOption { parcels: keys, ..Default::default() };
        let m = site_marks(&w, &site);
        assert_eq!(m.ground.len(), 3);
        assert!(m.buildings.is_empty() && m.circle.is_none());
        let spot = w.parcels.parcels[5].center;
        let site = nd_sim::committee::SiteOption { point: Some(spot), radius: 30.0, ..Default::default() };
        let m = site_marks(&w, &site);
        assert_eq!(m.ground.len(), 1, "the ground under the spot");
        assert_eq!(m.circle, Some((spot, 30.0)));
    }
}
