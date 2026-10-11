//! [committee] The Committee window: the meeting as it happens (typed out
//! line by line, with Skip), the mayor's vote, the six seats and the pool,
//! alerts, the projects waiting for review, and the backend settings.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use nd_minds::committee::driver::abstain_rest;
use nd_minds::committee::settings::BackendChoice;
use nd_minds::committee::{cast_mayor_vote, CommitteeSettings};
use nd_sim::committee::{CommitteeProject, Line, LineKind, MayorVote, Meeting, Politician, ProjectStatus, Speaker, Stage, SEATS};

use super::{pane, theme, UiFlags, GOLD, IVORY, WARN};
use crate::camera::{CameraFly, CameraFollow, CameraMode, FlyGoal};
use crate::committee::CommitteeRt;
use crate::sim::Sim;

const CYAN: egui::Color32 = egui::Color32::from_rgb(90, 210, 230);
const MAYOR: egui::Color32 = egui::Color32::from_rgb(240, 120, 200);
const OK: egui::Color32 = super::theme::GOOD_LIGHT;
/// Characters typed per second while a meeting is shown.
const TYPE_RATE: f32 = 260.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Meeting,
    Members,
    Alerts,
    Projects,
    Settings,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Meeting, Tab::Members, Tab::Alerts, Tab::Projects, Tab::Settings];

    fn label(self) -> &'static str {
        match self {
            Tab::Meeting => "Meeting",
            Tab::Members => "Members",
            Tab::Alerts => "Alerts",
            Tab::Projects => "Projects",
            Tab::Settings => "Settings",
        }
    }

    pub fn parse(s: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.label().eq_ignore_ascii_case(s.trim()))
    }
}

fn open_id() -> egui::Id {
    egui::Id::new("committee_window_open")
}

fn tab_id() -> egui::Id {
    egui::Id::new("committee_window_tab")
}

pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id()).unwrap_or(false))
}

pub fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

/// The top-bar toggle.
pub fn toggle(ui: &mut egui::Ui) {
    let open = is_open(ui.ctx());
    if ui.selectable_label(open, "Committee").on_hover_text("Your committee: seats, alerts, meetings and the projects they pass").clicked() {
        set_open(ui.ctx(), !open);
    }
}

/// Things the window asks of the world after drawing.
#[derive(Default)]
struct Asks {
    fly: Option<bevy::math::Vec2>,
    message: Option<String>,
    /// [fix/roads-tools] A project's Show: outline what it would change.
    show: Option<u32>,
}

#[allow(clippy::too_many_arguments)]
pub fn window_system(mut contexts: EguiContexts, mut sim: ResMut<Sim>, mut rt: ResMut<CommitteeRt>, mut flags: ResMut<UiFlags>, (mut fly, mut follow, mut mode): (ResMut<CameraFly>, ResMut<CameraFollow>, ResMut<CameraMode>), time: Res<Time>) -> Result {
    let ctx = contexts.ctx_mut()?;
    if let Some(open) = rt.open_request.take() {
        set_open(ctx, open);
        if open && sim.world.committee.meeting.is_some() {
            ctx.data_mut(|d| d.insert_temp(tab_id(), Tab::Meeting));
        }
    }
    if let Some(t) = rt.tab_request.take() {
        ctx.data_mut(|d| d.insert_temp(tab_id(), t));
    }
    if !is_open(ctx) {
        return Ok(());
    }
    let content = flags.map_rect(ctx.viewport_rect());
    let mut keep = true;
    let mut asks = Asks::default();
    let size = egui::vec2(640.0f32.min(content.width() - 24.0), 620.0f32.min(content.height() - 24.0));
    egui::Window::new("Committee")
        .id(egui::Id::new("committee_window"))
        .title_bar(false)
        .resizable(true)
        .constrain_to(content)
        .default_pos(egui::pos2(content.right() - size.x - 12.0, content.top() + 12.0))
        .default_size(size)
        .min_size(egui::vec2(460.0, 320.0))
        .show(ctx, |ui| {
            let note = sim.world.committee.meeting.as_ref().map(|m| format!("· in session: meeting {}, {}", m.number, m.stage.label()));
            keep = pane::title_bar(ui, "Committee", note.as_deref());
            let mut tab: Tab = ui.data(|d| d.get_temp(tab_id())).unwrap_or_default();
            ui.horizontal(|ui| {
                for t in Tab::ALL {
                    let mut label = t.label().to_string();
                    if t == Tab::Alerts {
                        let n = sim.world.committee.unseen_alerts();
                        if n > 0 {
                            label = format!("Alerts ({n})");
                        }
                    }
                    if t == Tab::Projects {
                        let n = sim.world.committee.projects.iter().filter(|p| p.status.is_open()).count();
                        if n > 0 {
                            label = format!("Projects ({n})");
                        }
                    }
                    if ui.selectable_label(tab == t, egui::RichText::new(label).font(theme::button())).clicked() {
                        tab = t;
                    }
                }
            });
            ui.separator();
            ui.data_mut(|d| d.insert_temp(tab_id(), tab));
            egui::ScrollArea::vertical().id_salt(("committee_scroll", tab as u8)).auto_shrink([false, false]).stick_to_bottom(tab == Tab::Meeting && sim.world.committee.meeting.is_some()).show(ui, |ui| match tab {
                Tab::Meeting => meeting_tab(ui, &mut sim, &mut rt, &mut asks),
                Tab::Members => members_tab(ui, &mut sim, &mut asks),
                Tab::Alerts => alerts_tab(ui, &mut sim),
                Tab::Projects => projects_tab(ui, &mut sim, &mut asks),
                Tab::Settings => settings_tab(ui, &mut sim, &mut rt),
            });
        });
    if !keep {
        set_open(ctx, false);
    }
    if let Some(m) = asks.message {
        flags.message = m;
    }
    if let Some(id) = asks.show {
        rt.shown_site = Some((id, time.elapsed_secs_f64()));
    }
    if let Some(p) = asks.fly {
        follow.stop();
        *mode = CameraMode::Orbit;
        fly.to(FlyGoal::Point(p), Some(160.0), Some(0.9));
    }
    Ok(())
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).font(theme::lead()).color(GOLD));
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().weak());
}

fn speaker_color(s: Speaker) -> egui::Color32 {
    match s {
        Speaker::Secretary => CYAN,
        Speaker::Member(_) => GOLD,
        Speaker::Mayor => MAYOR,
    }
}

/// One transcript line, `chars` of it (None: all).
fn line(ui: &mut egui::Ui, l: &Line, scope: Option<&str>, chars: Option<usize>) {
    let text: String = match chars {
        Some(n) => l.text.chars().take(n).collect(),
        None => l.text.clone(),
    };
    match l.kind {
        LineKind::Agenda => {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(text).font(theme::heading()).color(CYAN));
        }
        LineKind::Note => {
            ui.label(egui::RichText::new(text).small().italics().color(egui::Color32::from_gray(170)));
        }
        LineKind::Result => {
            let c = if l.text.contains("passed") { OK } else { WARN };
            ui.label(egui::RichText::new(text).strong().color(c));
        }
        _ => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(egui::RichText::new(&l.name).font(theme::button()).color(speaker_color(l.speaker)));
                if let Some(s) = scope {
                    ui.label(egui::RichText::new(format!("({s})")).small().weak());
                }
                let body = egui::RichText::new(text).color(IVORY);
                ui.label(if l.kind == LineKind::Vote { body.italics() } else { body });
            });
        }
    }
}

fn transcript(ui: &mut egui::Ui, sim: &Sim, m: &Meeting, shown: Option<(usize, usize)>) {
    let scope_of = |l: &Line| match l.speaker {
        Speaker::Member(id) => sim.world.committee.member(id).map(|p| p.scope.label()),
        _ => None,
    };
    for (i, l) in m.transcript.iter().enumerate() {
        match shown {
            Some((lines, chars)) if i == lines => {
                line(ui, l, scope_of(l), Some(chars));
                break;
            }
            Some((lines, _)) if i > lines => break,
            _ => line(ui, l, scope_of(l), None),
        }
    }
}

fn meeting_tab(ui: &mut egui::Ui, sim: &mut Sim, rt: &mut CommitteeRt, asks: &mut Asks) {
    let today = sim.world.committee_today();
    let Some(m) = sim.world.committee.meeting.as_ref() else {
        // Between meetings.
        let c = &sim.world.committee;
        ui.horizontal(|ui| {
            pane::kpi(ui, "next meeting", egui::RichText::new(if c.is_set_up() { format!("day {}", c.next_meeting_day + 1) } else { "not yet".into() }).color(IVORY));
            pane::kpi(ui, "meetings held", egui::RichText::new(c.meetings_held.to_string()).color(IVORY));
            pane::kpi(ui, "members seated", egui::RichText::new(format!("{}/{SEATS}", c.seated_ids().len())).color(IVORY));
            pane::kpi(ui, "speaking through", egui::RichText::new(rt.courier.backend_name()).color(IVORY));
        });
        if ui.button(egui::RichText::new("Call a meeting now").font(theme::button())).on_hover_text("The sim pauses while the committee sits").clicked() {
            match rt.call_meeting(sim) {
                Ok(n) => asks.message = Some(format!("Meeting {n} called")),
                Err(e) => asks.message = Some(format!("No meeting: {e}")),
            }
        }
        let auto = sim.world.committee.auto_meetings;
        hint(ui, &format!("Day {}. Meetings come every 30 days{}.", today + 1, if auto { "" } else { " (switched off: only when you call one)" }));
        if let Some(last) = sim.world.committee.minutes.last() {
            ui.separator();
            heading(ui, &format!("Minutes of meeting {} (day {})", last.number, last.day + 1));
            hint(ui, &format!("{} backend calls", last.calls));
            transcript(ui, sim, last, None);
        } else {
            hint(ui, "No meeting yet. The committee reads the city report (people, money, housing, transit, safety, power, ground and every area) and argues over real numbers.");
        }
        return;
    };
    // In session: type it out.
    if rt.fast_forward {
        rt.shown = (m.transcript.len(), 0);
    } else {
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        let mut budget = (TYPE_RATE * dt).ceil() as usize;
        while budget > 0 && rt.shown.0 < m.transcript.len() {
            let len = m.transcript[rt.shown.0].text.chars().count();
            let take = (len - rt.shown.1.min(len)).min(budget);
            rt.shown.1 += take;
            budget -= take;
            if rt.shown.1 >= len {
                rt.shown = (rt.shown.0 + 1, 0);
                budget = budget.saturating_sub(20);
            }
        }
        if rt.shown.0 < m.transcript.len() {
            ui.ctx().request_repaint();
        }
    }
    let typing = rt.shown.0 < m.transcript.len();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Meeting {} · day {} · {}", m.number, m.day + 1, m.stage.label())).font(theme::lead()).color(GOLD));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if (typing || !rt.fast_forward) && ui.button("Skip >>").on_hover_text("Show everything said so far, and the rest as it comes").clicked() {
                rt.fast_forward = true;
            }
        });
    });
    hint(ui, &format!("{} · {} calls so far{}", rt.courier.backend_name(), m.calls, if rt.driver.busy() { " · waiting for the next speaker..." } else { "" }));
    ui.separator();
    transcript(ui, sim, m, if rt.fast_forward { None } else { Some(rt.shown) });
    if typing && !rt.fast_forward {
        return;
    }
    // The mayor's clicks, cast once the meeting is no longer borrowed.
    let mut votes: Vec<(u32, MayorVote)> = Vec::new();
    let mut abstain = false;
    if matches!(m.stage, Stage::MayorVote | Stage::Votes) && !m.proposals.is_empty() {
        ui.add_space(8.0);
        heading(ui, "Your vote, mayor");
        for p in &m.proposals {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new(format!("#{} {}", p.id, p.title)).font(theme::button()).color(IVORY));
                hint(ui, &format!("tabled by {}{}", sim.world.committee.name_of(p.by), if p.cost > 0.0 { format!(" · about {} credits", nd_sim::committee::report::thousands(p.cost)) } else { String::new() }));
                let count = |v: nd_sim::committee::Vote| p.ballots.iter().filter(|b| b.vote == v).count();
                hint(ui, &format!("members: {} yes, {} no, {} abstaining", count(nd_sim::committee::Vote::Yes), count(nd_sim::committee::Vote::No), count(nd_sim::committee::Vote::Abstain)));
                ui.horizontal(|ui| {
                    for v in [MayorVote::Yes, MayorVote::No, MayorVote::Abstain, MayorVote::Veto] {
                        if v == MayorVote::Veto && !sim.world.committee.veto_allowed {
                            continue;
                        }
                        let on = p.mayor == Some(v);
                        let label = match v {
                            MayorVote::Yes => "Yes",
                            MayorVote::No => "No",
                            MayorVote::Abstain => "Abstain",
                            MayorVote::Veto => "Veto",
                        };
                        if ui.selectable_label(on, egui::RichText::new(label).font(theme::button())).clicked() {
                            votes.push((p.id, v));
                        }
                    }
                });
            });
        }
        if m.stage == Stage::MayorVote && m.waiting_for_mayor() && ui.button("Abstain on the rest").clicked() {
            abstain = true;
        }
    }
    for (id, v) in votes {
        if let Err(e) = cast_mayor_vote(&mut sim.world, id, v) {
            asks.message = Some(e);
        }
    }
    if abstain {
        let _ = abstain_rest(&mut sim.world);
    }
}

fn member_card(ui: &mut egui::Ui, p: &Politician, seat: Option<usize>, today: u32) -> Option<bool> {
    let mut act = None;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            if let Some(s) = seat {
                ui.label(egui::RichText::new(format!("Seat {}", s + 1)).small().color(CYAN));
            }
            ui.label(egui::RichText::new(&p.name).font(theme::lead()).color(IVORY));
            ui.label(egui::RichText::new(p.scope.title()).color(GOLD));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if seat.is_some() {
                    if ui.button("Dismiss").clicked() {
                        act = Some(false);
                    }
                } else if ui.button("Appoint").on_hover_text("To the first empty seat (dismiss someone to free one)").clicked() {
                    act = Some(true);
                }
            });
        });
        hint(ui, &format!("{} · {} · {} · age {}", p.temper.label(), p.party, p.leanings.describe(), p.age));
        ui.label(egui::RichText::new(p.bio()).small());
        let r = &p.record;
        let since = r.seated_since.map_or(String::new(), |d| format!("seated {} days · ", today.saturating_sub(d)));
        let n = |k: u32, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
        hint(ui, &format!("{since}{} · {} · tabled {} ({} passed) · voted {} yes, {} no, {} abstained", n(r.meetings, "meeting", "meetings"), n(r.alerts, "alert", "alerts"), r.proposed, r.proposals_passed, r.votes_yes, r.votes_no, r.abstained));
        if !p.memory.is_empty() {
            egui::CollapsingHeader::new(egui::RichText::new(format!("Remembers ({})", p.memory.len())).small()).id_salt(("mem", p.id)).show(ui, |ui| {
                for m in p.memory.iter().rev() {
                    ui.label(egui::RichText::new(m.line()).small());
                }
            });
        }
    });
    act
}

fn members_tab(ui: &mut egui::Ui, sim: &mut Sim, asks: &mut Asks) {
    sim.world.committee_ensure();
    let today = sim.world.committee_today();
    heading(ui, "The table");
    let c = &sim.world.committee;
    let mut action: Option<(u32, bool)> = None;
    for (i, seat) in c.seats.iter().enumerate() {
        match seat.and_then(|id| c.member(id)) {
            Some(p) => {
                if let Some(a) = member_card(ui, p, Some(i), today) {
                    action = Some((p.id, a));
                }
            }
            None => {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new(format!("Seat {}: empty", i + 1)).color(WARN));
                    hint(ui, "Appoint a candidate below. An empty seat raises no alerts and casts no vote.");
                });
            }
        }
    }
    heading(ui, "Candidates");
    hint(ui, "Every politician is fictional. Appointing fills the first empty seat.");
    for p in c.candidates() {
        if let Some(a) = member_card(ui, p, None, today) {
            action = Some((p.id, a));
        }
    }
    if let Some((id, appoint)) = action {
        let name = sim.world.committee.name_of(id);
        let r = if appoint { sim.world.committee.appoint(id, None, today).map(|s| format!("{name} takes seat {}", s + 1)) } else { sim.world.committee.dismiss(id, today).map(|s| format!("{name} leaves seat {}", s + 1)) };
        asks.message = Some(r.unwrap_or_else(|e| e));
    }
}

fn alerts_tab(ui: &mut egui::Ui, sim: &mut Sim) {
    let c = &sim.world.committee;
    if c.alerts.is_empty() {
        hint(ui, "No alerts yet. Between meetings each member watches their scope with plain rules (riders down 20%, a building without power, rent burden, crime up, ground contamination, a deficit...) and tells you when one trips.");
        return;
    }
    for a in c.alerts.iter().rev() {
        let who = c.member(a.member);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let col = match a.severity {
                    3 => WARN,
                    2 => GOLD,
                    _ => egui::Color32::from_gray(170),
                };
                ui.label(egui::RichText::new(["", "notice", "warning", "urgent"][a.severity.min(3) as usize]).small().strong().color(col));
                ui.label(egui::RichText::new(format!("day {}", a.day + 1)).small().weak());
                ui.label(egui::RichText::new(who.map_or("someone", |p| p.name.as_str())).font(theme::button()).color(GOLD));
                if let Some(p) = who {
                    ui.label(egui::RichText::new(p.scope.label()).small().weak());
                }
                if !a.seen {
                    ui.label(egui::RichText::new("new").small().color(CYAN));
                }
            });
            ui.label(egui::RichText::new(&a.text).color(IVORY));
            hint(ui, &a.facts.iter().map(|f| f.line()).collect::<Vec<_>>().join(" · "));
        });
    }
    for a in sim.world.committee.alerts.iter_mut() {
        a.seen = true;
    }
}

fn project_card(ui: &mut egui::Ui, sim: &Sim, p: &CommitteeProject) -> Option<ProjectAct> {
    let mut act = None;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&p.name).font(theme::lead()).color(IVORY));
            let (txt, col) = match &p.status {
                ProjectStatus::Pending => ("waiting for your review".to_string(), GOLD),
                ProjectStatus::NeedsMayor => ("needs you: no valid site".to_string(), WARN),
                ProjectStatus::Implemented { day } => (format!("implemented on day {}", day + 1), OK),
                ProjectStatus::Dismissed { day } => (format!("dismissed on day {}", day + 1), egui::Color32::from_gray(150)),
                ProjectStatus::Failed { why } => (format!("refused: {why}"), WARN),
            };
            ui.label(egui::RichText::new(txt).small().color(col));
        });
        hint(ui, &format!("meeting {} · tabled by {} · {}", p.meeting, sim.world.committee.name_of(p.by), p.action.kind_key().replace('_', " ")));
        if let Some(s) = p.site() {
            ui.label(egui::RichText::new(format!("Site: {}", s.label)).color(IVORY));
        }
        if !p.reason.is_empty() && p.pick.is_some() {
            ui.label(egui::RichText::new(format!("Secretary: {}", p.reason)).small().italics());
        }
        if p.status.is_open() && p.options.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                hint(ui, "Other options:");
                for (i, o) in p.options.iter().enumerate() {
                    if ui.selectable_label(p.pick == Some(i), format!("{}", i + 1)).on_hover_text(format!("{}\n{}", o.label, o.notes.join("\n"))).clicked() {
                        act = Some(ProjectAct::Pick(i));
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if p.site().and_then(|s| site_centre(sim, s)).is_some() && ui.button("Show").on_hover_text("Fly there and outline the ground and buildings it would change").clicked() {
                act = Some(ProjectAct::Show);
            }
            if p.status.is_open() {
                if ui.button(egui::RichText::new("Implement").font(theme::button()).color(OK)).on_hover_text("Carry it out now: the committee's only way into the city").clicked() {
                    act = Some(ProjectAct::Implement);
                }
                if ui.button("Look for sites again").clicked() {
                    act = Some(ProjectAct::Refresh);
                }
                if ui.button("Dismiss").clicked() {
                    act = Some(ProjectAct::Dismiss);
                }
            }
        });
        if p.world_project.is_some() {
            hint(ui, "The zoning is also in your projects list, previewed dimmed on the map.");
        }
        for l in p.log.iter().rev().take(3) {
            hint(ui, l);
        }
    });
    act
}

/// [fix/roads-tools] Where a site is: its building's spot, else its first
/// stop, else the middle of the parcels it zones.
fn site_centre(sim: &Sim, s: &nd_sim::committee::SiteOption) -> Option<nd_sim::Vec2> {
    if let Some(p) = s.point.or(s.stops.first().copied()) {
        return Some(p);
    }
    let pts: Vec<nd_sim::Vec2> = s.parcels.iter().filter_map(|k| sim.world.parcels.find(k)).map(|i| sim.world.parcels.parcels[i].center).collect();
    (!pts.is_empty()).then(|| pts.iter().copied().sum::<nd_sim::Vec2>() / pts.len() as f32)
}

enum ProjectAct {
    Show,
    Implement,
    Dismiss,
    Refresh,
    Pick(usize),
}

fn projects_tab(ui: &mut egui::Ui, sim: &mut Sim, asks: &mut Asks) {
    let projects = &sim.world.committee.projects;
    if projects.is_empty() {
        hint(ui, "Nothing yet. A proposal the committee passes comes here as a named project: the secretary picks a site among those the game finds valid, and you implement it, pick another site, or dismiss it.");
        return;
    }
    let mut todo: Option<(u32, ProjectAct)> = None;
    for open in [true, false] {
        heading(ui, if open { "Waiting for you" } else { "Done" });
        for p in projects.iter().rev().filter(|p| p.status.is_open() == open) {
            if let Some(a) = project_card(ui, sim, p) {
                todo = Some((p.id, a));
            }
        }
    }
    let Some((id, act)) = todo else { return };
    let w = &mut sim.world;
    let name = w.committee.project(id).map(|p| p.name.clone()).unwrap_or_default();
    asks.message = Some(match act {
        ProjectAct::Show => {
            let at = sim.world.committee.project(id).and_then(|p| p.site()).and_then(|s| site_centre(sim, s));
            if let Some(pt) = at {
                asks.fly = Some(bevy::math::Vec2::new(pt.x, pt.y));
            }
            asks.show = Some(id);
            return;
        }
        ProjectAct::Implement => match w.committee_implement(id) {
            Ok(done) => format!("{name}: {done}"),
            Err(e) => format!("{name} could not be implemented: {e}"),
        },
        ProjectAct::Dismiss => w.committee_dismiss_project(id).map_or_else(|e| e, |_| format!("{name} dismissed")),
        ProjectAct::Refresh => w.committee_refresh_sites(id).map_or_else(|e| e, |n| format!("{name}: {n} valid sites found")),
        ProjectAct::Pick(i) => w.committee_pick_site(id, i).map_or_else(|e| e, |_| format!("{name}: option {} picked", i + 1)),
    });
}

fn settings_tab(ui: &mut egui::Ui, sim: &mut Sim, rt: &mut CommitteeRt) {
    heading(ui, "Who speaks for the committee");
    let mut s: CommitteeSettings = rt.settings.clone();
    for b in BackendChoice::ALL {
        ui.radio_value(&mut s.backend, b, b.label());
    }
    match s.backend {
        BackendChoice::Mock => hint(ui, "Templates and rules with the city's real numbers. Offline, free and deterministic."),
        BackendChoice::Local => {
            hint(ui, "Any OpenAI-compatible server: Ollama (http://127.0.0.1:11434/v1), LM Studio (http://127.0.0.1:1234/v1), llama.cpp, vLLM. A 14-32B model keeps to its role best.");
            egui::Grid::new("local_settings").num_columns(2).show(ui, |ui| {
                ui.label("URL");
                ui.text_edit_singleline(&mut s.local_url);
                ui.end_row();
                ui.label("Model");
                ui.text_edit_singleline(&mut s.local_model);
                ui.end_row();
            });
        }
        BackendChoice::Claude => {
            hint(ui, "The Claude Messages API. About 22 short calls per meeting; alerts are one call each.");
            egui::Grid::new("claude_settings").num_columns(2).show(ui, |ui| {
                ui.label("Model");
                ui.text_edit_singleline(&mut s.claude_model);
                ui.end_row();
                ui.label("Effort");
                egui::ComboBox::from_id_salt("claude_effort").selected_text(s.claude_effort.clone()).show_ui(ui, |ui| {
                    for e in ["low", "medium", "high"] {
                        ui.selectable_value(&mut s.claude_effort, e.to_string(), e);
                    }
                });
                ui.end_row();
                ui.label("API key");
                ui.label(egui::RichText::new(s.key_source().label()).small());
                ui.end_row();
                ui.label("");
                let mut key = s.claude_api_key.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut key).password(true).hint_text("paste a key to keep in committee_settings.ron")).changed() {
                    s.claude_api_key = (!key.trim().is_empty()).then(|| key.trim().to_string());
                }
                ui.end_row();
            });
            ui.checkbox(&mut s.claude_fallbacks, "Let the API re-run a declined turn on a fallback model").on_hover_text("fallbacks: \"default\"; switch off if your model rejects it");
        }
    }
    ui.horizontal(|ui| {
        ui.label("Timeout (s)");
        ui.add(egui::DragValue::new(&mut s.timeout_s).range(5..=600));
        ui.label("Reply cap (tokens)");
        ui.add(egui::DragValue::new(&mut s.max_tokens).range(64..=4000));
    });
    if s != rt.settings {
        rt.settings = s;
    }
    ui.horizontal(|ui| {
        if ui.button(egui::RichText::new("Apply and save").font(theme::button())).clicked() {
            rt.apply_settings();
            if let Err(e) = rt.save_settings() {
                rt.backend_error = Some(e);
            }
        }
        if ui.add_enabled(rt.test.is_none(), egui::Button::new("Test connection")).clicked() {
            rt.test_connection();
        }
    });
    if rt.test.is_some() {
        hint(ui, "Testing...");
    }
    match &rt.test_result {
        Some(Ok(t)) => {
            ui.label(egui::RichText::new(t).color(OK));
        }
        Some(Err(e)) => {
            ui.label(egui::RichText::new(e).color(WARN));
        }
        None => {}
    }
    ui.label(egui::RichText::new(format!("Speaking through: {}", rt.courier.backend_name())).color(IVORY));
    if let Some(e) = &rt.backend_error {
        ui.label(egui::RichText::new(format!("{e}. The stand-in speaks until this is fixed.")).color(WARN));
    }
    hint(ui, &format!("{} calls made, {} answered by the stand-in after a failure", rt.courier.calls, rt.courier.fallbacks));
    ui.checkbox(&mut rt.remote_may_switch, "Let the remote API change the backend (this run only)")
        .on_hover_text("Off: scripts and agents driving the game may read these settings and test the connection, but not switch the committee to a paid model or another server");
    for e in rt.courier.errors.iter().rev().take(3) {
        ui.label(egui::RichText::new(e).small().color(WARN));
    }
    heading(ui, "Rules of the house");
    let c = &mut sim.world.committee;
    ui.checkbox(&mut c.auto_meetings, "Meet every 30 days (off: only when you call a meeting)");
    ui.checkbox(&mut c.veto_allowed, "The mayor may veto");
    hint(ui, "Whatever backend speaks, proposals are checked against what the game can do, sites come from the game, and nothing is built until you implement it.");
}
