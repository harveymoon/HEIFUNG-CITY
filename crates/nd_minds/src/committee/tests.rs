//! [committee] Meetings, votes, projects, memory through a save, and the
//! connectors against a fake model server on localhost. No real network
//! service is called.

use std::sync::OnceLock;
use std::time::Duration;

use nd_sim::committee::proposals::site_options;
use nd_sim::committee::{Action, LineKind, MayorVote, Outcome, ProjectStatus, Stage};
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, LandUse, PowerPlantKind, Vec2, World, WorldConfig};

use super::driver::abstain_rest;
use super::http::{backend_from, HttpBackend};
use super::settings::{BackendChoice, CommitteeSettings};
use super::*;
use crate::RuleBasedMind;

/// A small grid town: most of it zoned, the rest free ground along the
/// streets, a coal plant, no depot, no police or clinic; grown three days.
fn grown() -> World {
    static W: OnceLock<World> = OnceLock::new();
    W.get_or_init(|| {
        let mut w = World::new(WorldConfig::default());
        w.drain_events();
        w.start_project();
        for i in -2..=2 {
            let c = i as f32 * 140.0;
            w.add_gesture(Gesture::Road { points: vec![Vec2::new(-280.0, c), Vec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
            w.add_gesture(Gesture::Road { points: vec![Vec2::new(c, -280.0), Vec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        }
        let cell = |cx: f32, cy: f32| vec![Vec2::new(cx - 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy + 65.0), Vec2::new(cx - 65.0, cy + 65.0)];
        let plan = [
            (-210.0, -210.0, LandUse::Industrial),
            (-70.0, -210.0, LandUse::Industrial),
            (70.0, -210.0, LandUse::Residential),
            (-210.0, -70.0, LandUse::Commercial),
            (-70.0, -70.0, LandUse::Entertainment),
            (70.0, -70.0, LandUse::Commercial),
            (210.0, -70.0, LandUse::Residential),
            (-210.0, 70.0, LandUse::Residential),
            (-70.0, 70.0, LandUse::Residential),
            (70.0, 70.0, LandUse::Commercial),
        ];
        for (x, y, lu) in plan {
            w.add_gesture(Gesture::Zone { polygon: cell(x, y), land_use: lu });
        }
        w.implement();
        w.economy.treasury = 200_000.0;
        w.link_to_highway(Vec2::new(0.0, 280.0));
        let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.area() >= 600.0).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id);
        if let Some(lot) = lot {
            let _ = w.build_power(lot, PowerPlantKind::Coal);
        }
        let mut minds = RuleBasedMind::default();
        for _ in 0..3 * 1440 {
            w.tick(&mut minds);
        }
        w.drain_events();
        w.economy.treasury = w.economy.treasury.max(200_000.0);
        w
    })
    .clone()
}

/// Run a meeting with the stand-in up to the mayor's vote.
fn meet(w: &mut World) -> (Driver, Courier) {
    let mut d = Driver::default();
    let mut c = Courier::mock();
    start_meeting(w, true).expect("a meeting starts");
    let _ = d.run(w, &mut c, 400);
    (d, c)
}

#[test]
fn a_meeting_runs_the_agenda_and_waits_for_the_mayor() {
    let mut w = grown();
    assert!(w.citizens.len() > 150, "the town grew: {}", w.citizens.len());
    let (mut d, mut c) = meet(&mut w);
    let m = w.committee.meeting.as_ref().unwrap();
    assert_eq!(m.stage, Stage::MayorVote, "{:#?}", m.transcript);
    assert!(!m.proposals.is_empty(), "the rules table something in a town with no police, clinic or depot");
    assert!(m.proposals.len() <= nd_sim::committee::MAX_PROPOSALS);
    for p in &m.proposals {
        assert_eq!(p.ballots.len(), m.members.len(), "every member voted on #{}", p.id);
        assert!(p.action.validate(&w).is_ok(), "tabled proposals validate: {:?}", p.action);
    }
    let speeches = m.transcript.iter().filter(|l| l.kind == LineKind::Speech).count();
    assert!(speeches >= m.members.len() + 2, "opening, a report each, the topics");
    assert!(m.calls <= 22, "turn limits: {} calls", m.calls);
    abstain_rest(&mut w).unwrap();
    let _ = d.run(&mut w, &mut c, 400);
    assert!(w.committee.meeting.is_none(), "the meeting closed and was filed");
    let minutes = w.committee.minutes.last().unwrap();
    assert_eq!(minutes.stage, Stage::Done);
    assert!(minutes.proposals.iter().all(|p| p.outcome.is_some()));
    // Every member remembers how they voted.
    for id in &minutes.members {
        let p = w.committee.member(*id).unwrap();
        assert_eq!(p.memory.len(), minutes.proposals.len(), "{} remembers", p.name);
    }
    println!("{}", minutes.transcript.iter().map(|l| format!("{}: {}", l.name, l.text)).collect::<Vec<_>>().join("\n"));
}

#[test]
fn the_stand_in_meeting_is_deterministic() {
    let run = || {
        let mut w = grown();
        let (mut d, mut c) = meet(&mut w);
        let ids: Vec<u32> = w.committee.meeting.as_ref().unwrap().proposals.iter().map(|p| p.id).collect();
        for id in ids {
            cast_mayor_vote(&mut w, id, MayorVote::Yes).unwrap();
        }
        let _ = d.run(&mut w, &mut c, 400);
        (w.committee.minutes.last().unwrap().clone(), w.committee.projects.clone())
    };
    let (a, pa) = run();
    let (b, pb) = run();
    assert_eq!(a.transcript, b.transcript);
    assert_eq!(a.proposals, b.proposals);
    assert_eq!(pa, pb);
}

#[test]
fn the_mayor_vote_and_veto_decide_close_calls() {
    use nd_sim::committee::Vote;
    let mut w = grown();
    let _ = meet(&mut w);
    let m = w.committee.meeting.as_mut().unwrap();
    let p = &mut m.proposals[0];
    // Rig a tie among the members: the mayor's vote breaks it.
    let n = p.ballots.len();
    for (i, b) in p.ballots.iter_mut().enumerate() {
        b.vote = if i < n / 2 { Vote::Yes } else { Vote::No };
    }
    if n % 2 == 1 {
        p.ballots.last_mut().unwrap().vote = Vote::Abstain;
    }
    p.mayor = Some(MayorVote::Yes);
    assert_eq!(p.tally().1, Outcome::Passed);
    p.mayor = Some(MayorVote::No);
    assert_eq!(p.tally().1, Outcome::Failed);
    p.mayor = Some(MayorVote::Abstain);
    assert_eq!(p.tally().1, Outcome::Failed, "a tie fails");
    // Unanimous members, vetoed.
    for b in p.ballots.iter_mut() {
        b.vote = Vote::Yes;
    }
    p.mayor = Some(MayorVote::Veto);
    let (t, o) = p.tally();
    assert_eq!(o, Outcome::Vetoed);
    assert_eq!(t.yes as usize, n);
    // The veto can be switched off.
    w.committee.veto_allowed = false;
    let id = w.committee.meeting.as_ref().unwrap().proposals[0].id;
    assert!(cast_mayor_vote(&mut w, id, MayorVote::Veto).is_err());
    assert!(cast_mayor_vote(&mut w, id, MayorVote::Yes).is_ok());
    assert!(cast_mayor_vote(&mut w, 999, MayorVote::Yes).is_err());
}

#[test]
fn passed_proposals_become_projects_on_valid_sites_and_the_mayor_implements_them() {
    let mut w = grown();
    let (mut d, mut c) = meet(&mut w);
    let ids: Vec<u32> = w.committee.meeting.as_ref().unwrap().proposals.iter().map(|p| p.id).collect();
    for id in &ids {
        cast_mayor_vote(&mut w, *id, MayorVote::Yes).unwrap();
    }
    let _ = d.run(&mut w, &mut c, 400);
    let minutes = w.committee.minutes.last().unwrap().clone();
    let passed: Vec<_> = minutes.proposals.iter().filter(|p| p.outcome == Some(Outcome::Passed)).collect();
    assert!(!passed.is_empty(), "with the mayor's yes something passes: {:#?}", minutes.transcript);
    for p in &passed {
        let pid = p.project.expect("a passed proposal is filed as a project");
        let proj = w.committee.project(pid).unwrap();
        assert!(proj.name.starts_with("Committee: "), "{}", proj.name);
        match proj.site() {
            Some(site) => {
                if let (Some(pt), Some(what)) = (site.point, placeable_of(&proj.action)) {
                    // Code-made sites: the game would accept the building there.
                    assert!(w.placement_problem_at(what, pt).is_ok(), "{} at {pt:?}", proj.name);
                }
            }
            None => assert_eq!(proj.status, ProjectStatus::NeedsMayor),
        }
    }
    // The mayor implements the first with a site: the city changes only now.
    let pid = passed.iter().filter_map(|p| p.project).find(|&id| w.committee.project(id).unwrap().site().is_some()).expect("some project has a site");
    let before = w.buildings.len();
    let building = placeable_of(&w.committee.project(pid).unwrap().action).is_some();
    let done = w.committee_implement(pid).expect("implements");
    let proj = w.committee.project(pid).unwrap().clone();
    assert!(matches!(proj.status, ProjectStatus::Implemented { .. }), "{done}");
    if building {
        assert_eq!(w.buildings.len(), before + 1, "{done}");
    }
    assert!(w.committee_implement(pid).is_err(), "once only");
    // Members who voted remember it was implemented.
    let title = proj.name.trim_start_matches("Committee: ").to_string();
    assert!(w.committee.pool.iter().any(|p| p.memory.iter().any(|m| m.proposal == title && m.outcome.contains("implemented"))));
}

fn placeable_of(a: &Action) -> Option<nd_sim::services::Placeable> {
    use nd_sim::services::{Placeable, ServiceKind};
    match a {
        Action::BuildDepot { .. } => Some(Placeable::BusDepot),
        Action::BuildService { service, tier, .. } => Some(Placeable::Service(ServiceKind::from_key(service)?, match tier.as_str() {
            "mid" => nd_sim::Tier::Mid,
            "high" => nd_sim::Tier::High,
            _ => nd_sim::Tier::Low,
        })),
        Action::BuildPower { plant, .. } => Some(Placeable::Plant(match plant.as_str() {
            "coal" => PowerPlantKind::Coal,
            "wind" => PowerPlantKind::Wind,
            _ => PowerPlantKind::Solar,
        })),
        _ => None,
    }
}

thread_local!(static LIES: std::cell::RefCell<Vec<(u64, TurnRequest)>> = const { std::cell::RefCell::new(Vec::new()) });

/// A model that tables a spaceport.
struct Liar;

impl CommitteeBackend for Liar {
    fn submit(&mut self, id: u64, req: TurnRequest) {
        LIES.with(|l| l.borrow_mut().push((id, req)));
    }
    fn poll(&mut self) -> Vec<(u64, Result<TurnReply, String>)> {
        LIES.with(|l| {
            l.borrow_mut()
                .drain(..)
                .map(|(id, req)| {
                    let raw = match req.task {
                        Task::Propose { .. } => r#"Here: {"proposal": {"action": "build_service", "service": "spaceport", "near": "Downtown"}, "rationale": "Rockets!"}"#.to_string(),
                        Task::Vote { .. } => r#"{"votes": []}"#.to_string(),
                        Task::Pick { .. } => r#"{"choice": 1, "reason": "first"}"#.to_string(),
                        _ => "Words.".to_string(),
                    };
                    (id, Ok(TurnReply::from_raw(&raw, req.task.wants_json())))
                })
                .collect()
        })
    }
    fn name(&self) -> String {
        "liar".into()
    }
}

#[test]
fn invalid_proposals_are_ruled_out_of_order() {
    let w = grown();
    let bad = [
        r#"{"action":"build_service","service":"spaceport","near":"Downtown"}"#,
        r#"{"action":"build_service","service":"police","near":"Atlantis"}"#,
        r#"{"action":"change_tax","category":"residential","delta_pct":40}"#,
        r#"{"action":"change_tax","category":"unicorns","delta_pct":1}"#,
        r#"{"action":"rezone","area":"Northside","to":"moon base"}"#,
        r#"{"action":"add_bus_line","from":"Northside","to":"Northside"}"#,
        r#"{"action":"set_toll","price":5}"#,
        r#"{"action":"road_repair","funding":9}"#,
        r#"{"action":"remediate","area":"Downtown"}"#,
    ];
    for j in bad {
        let a: Action = serde_json::from_str(j).unwrap();
        assert!(a.validate(&w).is_err(), "{j} should be refused");
    }
    assert!(serde_json::from_str::<Action>(r#"{"action":"launch_rocket"}"#).is_err());
    let good = [
        r#"{"action":"build_service","service":"Police","tier":"low","near":"north"}"#,
        r#"{"action":"rezone","area":"Southside","to":"residential"}"#,
        r#"{"action":"change_tax","category":"residential_mid","delta_pct":1}"#,
        r#"{"action":"road_repair","funding":1.3}"#,
    ];
    for j in good {
        let a: Action = serde_json::from_str(j).unwrap();
        let v = a.validate(&w).unwrap_or_else(|e| panic!("{j}: {e}"));
        if let Action::BuildService { service, near, .. } = &v {
            assert_eq!((service.as_str(), near.as_str()), ("police", "Northside"), "normalised");
        }
    }
    // A model's invalid proposal in a meeting is noted, not tabled.
    let mut w = grown();
    start_meeting(&mut w, true).unwrap();
    let mut d = Driver::default();
    let mut c = Courier::new(Box::new(Liar), Duration::from_secs(60));
    let _ = d.run(&mut w, &mut c, 400);
    let m = w.committee.meeting.as_ref().or(w.committee.minutes.last()).unwrap();
    assert!(m.proposals.is_empty(), "nothing valid was tabled: {:?}", m.proposals);
    assert!(m.transcript.iter().any(|l| l.kind == LineKind::Note && l.text.starts_with("Out of order")), "{:#?}", m.transcript);
}

#[test]
fn memory_and_meetings_survive_a_save() {
    let mut w = grown();
    let (mut d, mut c) = meet(&mut w);
    abstain_rest(&mut w).unwrap();
    let _ = d.run(&mut w, &mut c, 400);
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    assert_eq!(back.committee, w.committee);
    assert!(back.committee.pool.iter().any(|p| !p.memory.is_empty()));
    assert_eq!(back.committee.minutes.len(), 1);
}

#[test]
fn a_meeting_in_session_resumes_after_a_load() {
    let mut w = grown();
    start_meeting(&mut w, false).unwrap();
    let mut d = Driver::default();
    let mut c = Courier::mock();
    d.advance(&mut w, &mut c);
    for x in c.poll() {
        d.take(&mut w, &x);
    }
    let mut back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    let mut d2 = Driver::default();
    let mut c2 = Courier::mock();
    let _ = d2.run(&mut back, &mut c2, 400);
    assert_eq!(back.committee.meeting.as_ref().map(|m| m.stage), Some(Stage::MayorVote));
}

#[test]
fn sites_for_a_clinic_are_valid_and_ranked() {
    let w = grown();
    let a = Action::BuildService { service: "health".into(), tier: "low".into(), near: String::new() }.validate(&w).unwrap();
    let opts = site_options(&w, &a);
    assert!(!opts.is_empty(), "free ground exists");
    for o in &opts {
        assert!(w.placement_problem_at(nd_sim::services::Placeable::Service(nd_sim::services::ServiceKind::Health, nd_sim::Tier::Low), o.point.unwrap()).is_ok());
    }
    assert!(opts.windows(2).all(|p| p[0].score >= p[1].score));
    // Zoning finds free parcels in an area and stages them as a parked
    // world project the mayor sees with the others.
    let mut w = grown();
    let areas = nd_sim::committee::report::areas(&w);
    let area = areas.iter().find(|a| !site_options(&w, &Action::Rezone { area: a.name.clone(), to: "residential".into() }).is_empty()).expect("some area has free ground").name.clone();
    let action = Action::Rezone { area, to: "residential".into() }.validate(&w).unwrap();
    let opts = site_options(&w, &action);
    let prop = nd_sim::committee::Proposal { id: 1, by: 1, title: action.title(), action, rationale: String::new(), cost: 0.0, ballots: Vec::new(), mayor: None, outcome: Some(Outcome::Passed), project: None };
    let active_before = w.project.as_ref().map(|p| p.id);
    let pid = w.committee_file_project(1, &prop, opts, Some(0), "test".into());
    let wp = w.committee.project(pid).unwrap().world_project.expect("a world project");
    assert!(w.projects().iter().any(|p| p.id == wp && p.name.starts_with("Committee: ")));
    assert_eq!(w.project.as_ref().map(|p| p.id), active_before, "the mayor's active project is untouched");
    let parcels_before = w.parcels.parcels.iter().filter(|p| p.land_use == Some(LandUse::Residential)).count();
    w.committee_implement(pid).unwrap();
    let parcels_after = w.parcels.parcels.iter().filter(|p| p.land_use == Some(LandUse::Residential)).count();
    assert!(parcels_after > parcels_before, "zoned: {parcels_before} -> {parcels_after}");
}

#[test]
fn alerts_get_phrased_by_the_stand_in() {
    let mut w = grown();
    w.committee_ensure();
    let ids = w.committee_daily();
    assert!(!ids.is_empty(), "a town with no police or clinic raises something: {:?}", w.committee.alerts);
    let mut d = Driver::default();
    let mut c = Courier::mock();
    for id in &ids {
        d.phrase_alert(&w, *id, &mut c);
    }
    for x in c.poll() {
        assert!(d.take(&mut w, &x));
    }
    for id in ids {
        let a = w.committee.alerts.iter().find(|a| a.id == id).unwrap();
        assert!(a.phrased && !a.text.is_empty(), "{a:?}");
    }
}

// ── connectors, against a fake server on localhost ──────────────────────────

type Seen = (String, Vec<(String, String)>, String);

/// An HTTP server on localhost that answers every request with `body` and
/// hands back what it was sent.
fn fake_server(status: u16, body: &'static str) -> (String, std::sync::mpsc::Receiver<Seen>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for mut req in server.incoming_requests().take(64) {
            let mut sent = String::new();
            let _ = std::io::Read::read_to_string(req.as_reader(), &mut sent);
            let headers = req.headers().iter().map(|h| (h.field.to_string().to_ascii_lowercase(), h.value.to_string())).collect();
            let _ = tx.send((req.url().to_string(), headers, sent));
            let _ = req.respond(tiny_http::Response::from_string(body).with_status_code(status));
        }
    });
    (format!("http://127.0.0.1:{port}"), rx)
}

fn sample_request() -> TurnRequest {
    TurnRequest { speaker: None, task: Task::Opening { meeting: 1, members: vec!["A".into()], alerts: 0, on_demand: false }, city: "Testville".into(), day: 3, report: "CITY REPORT".into(), seed: 1, max_tokens: 100 }
}

fn wait(b: &mut HttpBackend) -> Result<TurnReply, String> {
    for _ in 0..500 {
        if let Some((_, r)) = b.poll().pop() {
            return r;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no reply");
}

#[test]
fn the_local_connector_speaks_openai_chat_completions() {
    let (url, rx) = fake_server(200, r#"{"choices":[{"message":{"role":"assistant","content":"<think>hm</think>Order, please."}}]}"#);
    let mut b = HttpBackend::new(http::Flavor::OpenAi { base_url: format!("{url}/v1"), model: "qwen-test".into(), max_tokens: 200 }, Duration::from_secs(5));
    b.submit(1, sample_request());
    let r = wait(&mut b).unwrap();
    assert_eq!(r.text, "Order, please.");
    let (path, _, sent) = rx.recv().unwrap();
    assert_eq!(path, "/v1/chat/completions");
    let v: serde_json::Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(v["model"], "qwen-test");
    assert_eq!(v["messages"][0]["role"], "system");
    assert!(v["messages"][1]["content"].as_str().unwrap().contains("CITY REPORT"));
}

/// [fix/review-app] A turn the courier gave up on is not sent once the
/// worker reaches it (a paid call whose answer would be thrown away).
#[test]
fn a_cancelled_turn_is_never_sent() {
    let (url, rx) = fake_server(200, r#"{"choices":[{"message":{"role":"assistant","content":"Noted."}}]}"#);
    let mut b = HttpBackend::new(http::Flavor::OpenAi { base_url: format!("{url}/v1"), model: "m".into(), max_tokens: 50 }, Duration::from_secs(5));
    b.cancel(2);
    for id in 1..=3 {
        b.submit(id, sample_request());
    }
    let mut got = Vec::new();
    for _ in 0..500 {
        got.extend(b.poll().into_iter().map(|(id, _)| id));
        if got.len() >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(100));
    got.extend(b.poll().into_iter().map(|(id, _)| id));
    assert_eq!(got, vec![1, 3]);
    assert_eq!(rx.try_iter().count(), 2, "the server saw two requests");
}

#[test]
fn the_claude_connector_speaks_the_messages_api_and_keeps_the_key_quiet() {
    let (url, rx) = fake_server(200, r#"{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"We are in session."}],"stop_reason":"end_turn"}"#);
    let key = "sk-test-not-a-real-key".to_string();
    let flavor = http::Flavor::Claude { url: format!("{url}/v1/messages"), model: "claude-sonnet-5-5".into(), key: key.clone(), effort: "low".into(), fallbacks: true, max_tokens: 1600 };
    assert!(!format!("{flavor:?}").contains(&key));
    let mut b = HttpBackend::new(flavor, Duration::from_secs(5));
    b.submit(7, sample_request());
    assert_eq!(wait(&mut b).unwrap().text, "We are in session.");
    let (path, headers, sent) = rx.recv().unwrap();
    assert_eq!(path, "/v1/messages");
    let h = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(h("x-api-key").as_deref(), Some(key.as_str()));
    assert_eq!(h("anthropic-version").as_deref(), Some("2023-06-01"));
    assert_eq!(h("anthropic-beta").as_deref(), Some("server-side-fallback-2026-07-01"));
    let v: serde_json::Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(v["model"], "claude-sonnet-5-5");
    assert!(v["system"].as_str().unwrap().contains("secretary"));
    assert_eq!(v["messages"][0]["role"], "user");
    assert!(v.get("temperature").is_none(), "no sampling parameters for Claude");
    assert_eq!(v["output_config"]["effort"], "low");
    assert_eq!(v["fallbacks"], "default");

    // Errors are reported with the API's message, never the key.
    let (url, _rx) = fake_server(401, r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#);
    let mut b = HttpBackend::new(http::Flavor::Claude { url: format!("{url}/v1/messages"), model: "m".into(), key: key.clone(), effort: String::new(), fallbacks: false, max_tokens: 100 }, Duration::from_secs(5));
    b.submit(8, sample_request());
    let e = wait(&mut b).unwrap_err();
    assert!(e.contains("401") && e.contains("invalid x-api-key") && !e.contains(&key), "{e}");

    // A declined turn is an error the stand-in answers.
    let (url, _rx) = fake_server(200, r#"{"content":[],"stop_reason":"refusal"}"#);
    let mut b = HttpBackend::new(http::Flavor::Claude { url: format!("{url}/v1/messages"), model: "m".into(), key, effort: String::new(), fallbacks: false, max_tokens: 100 }, Duration::from_secs(5));
    b.submit(9, sample_request());
    assert!(wait(&mut b).unwrap_err().contains("declined"));
}

#[test]
fn a_failing_backend_falls_back_to_the_stand_in() {
    // A server that fails every request.
    let (url, _rx) = fake_server(500, r#"{"error": "model not loaded"}"#);
    let s = CommitteeSettings { backend: BackendChoice::Local, local_url: format!("{url}/v1"), timeout_s: 5, ..Default::default() };
    let mut c = Courier::new(backend_from(&s).unwrap(), Duration::from_secs(5));
    let mut w = grown();
    start_meeting(&mut w, true).unwrap();
    let mut d = Driver::default();
    for _ in 0..2000 {
        d.advance(&mut w, &mut c);
        for x in c.poll() {
            d.take(&mut w, &x);
        }
        if w.committee.meeting.as_ref().is_some_and(|m| m.waiting_for_mayor() || m.is_done()) && !d.busy() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let m = w.committee.meeting.as_ref().unwrap();
    assert!(c.fallbacks > 0 && c.errors.iter().any(|e| e.contains("500") && e.contains("model not loaded")), "{:?}", c.errors);
    assert!(m.transcript.iter().any(|l| l.text.starts_with("(The model did not answer")));
    assert_eq!(m.stage, Stage::MayorVote, "{:?}", m.stage);
}

#[test]
fn settings_never_show_the_key_and_claude_needs_one() {
    let s = CommitteeSettings { claude_api_key: Some("sk-secret".into()), ..Default::default() };
    assert!(!format!("{s:?}").contains("sk-secret"));
    let dir = std::env::temp_dir().join(format!("nd_committee_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("committee_settings.ron");
    let s2 = CommitteeSettings { backend: BackendChoice::Local, local_model: "llama-test".into(), ..Default::default() };
    s2.save_to(&path).unwrap();
    assert_eq!(CommitteeSettings::load_from(&path), s2);
    assert_eq!(CommitteeSettings::load_from(&dir.join("missing.ron")), CommitteeSettings::default());
    let _ = std::fs::remove_dir_all(&dir);
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        let none = CommitteeSettings { backend: BackendChoice::Claude, ..Default::default() };
        assert!(backend_from(&none).is_err(), "no key, no Claude");
    }
    assert!(extract_json("Sure! ```json\n{\"choice\": 2, \"reason\": \"a {brace} in a string\"}\n```").is_some_and(|v| v["choice"] == 2));
    assert!(extract_json("no json here").is_none());
}

// ── voices: conversation lines and diaries ──────────────────────────────────

use super::voices::{clean_line, Voices};

#[test]
fn diaries_come_from_memories_and_the_stand_in_is_deterministic() {
    let w = grown();
    let id = w.citizens.iter().filter(|(_, c)| c.memories.len() >= 2).map(|(id, _)| id).next().expect("someone remembers something");
    let write = || {
        let mut v = Voices::default();
        let mut c = Courier::mock();
        v.ask_diary(&w, id, &mut c, false);
        assert!(v.diary(id).unwrap().pending);
        for d in c.poll() {
            assert!(v.take(&d));
        }
        v.diary(id).unwrap().clone()
    };
    let a = write();
    assert!(!a.pending && !a.by_model, "the stand-in wrote it");
    assert!(a.text.starts_with(&format!("Day {}.", w.clock.day() + 1)), "{}", a.text);
    assert!(a.text.len() > 40, "{}", a.text);
    assert_eq!(a, write(), "same city, same diary");
    println!("{}: {}", w.citizens[id].name, a.text);
}

#[test]
fn conversation_lines_are_asked_only_for_watched_people_and_never_touch_the_sim() {
    let w = grown();
    assert!(!w.social.recent.is_empty(), "people talked in three days");
    let before = w.to_ron().unwrap();
    // The stand-in keeps the sim's template: nothing is asked.
    let mut v = Voices::default();
    let mut c = Courier::mock();
    v.pump(&w, &mut c, &|_| true);
    assert_eq!(c.calls, 0);
    // A model is asked for lines of watched people only.
    let (url, rx) = fake_server(200, r#"{"choices":[{"message":{"content":"Kai: \"Rent here would make a saint swear.\""}}]}"#);
    let s = CommitteeSettings { backend: BackendChoice::Local, local_url: format!("{url}/v1"), ..Default::default() };
    let mut c = Courier::new(backend_from(&s).unwrap(), Duration::from_secs(10));
    let t = w.social.recent.back().unwrap().clone();
    let mut v = Voices::default();
    v.pump(&w, &mut c, &|x| x == t.a);
    let asked = c.calls;
    assert!(asked >= 1 && asked as usize <= super::voices::MAX_LINES_IN_FLIGHT);
    let mut got = 0;
    for _ in 0..500 {
        for d in c.poll() {
            assert!(v.take(&d));
            got += 1;
        }
        if got == asked {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let (_, _, sent) = rx.recv().unwrap();
    assert!(sent.contains("one line of dialogue"), "{sent}");
    let line = w.social.recent.iter().rev().find_map(|x| v.line(x.serial)).expect("a model line arrived");
    assert_eq!(line, "Rent here would make a saint swear.");
    // The sim's own line is untouched, and so is the world.
    assert_eq!(w.social.recent.back().unwrap().line, t.line);
    assert_eq!(w.to_ron().unwrap(), before);
    // Seen conversations are not asked again.
    let calls = c.calls;
    v.pump(&w, &mut c, &|_| true);
    assert_eq!(c.calls, calls);
}

#[test]
fn model_lines_are_cleaned() {
    assert_eq!(clean_line("Mira: \"Hello there.\""), "Hello there.");
    assert_eq!(clean_line("\n\n  Nice night.  \nSecond line"), "Nice night.");
    assert_eq!(clean_line("Note: this is a line: with colons"), "this is a line: with colons");
    let long = (0..40).map(|_| "word").collect::<Vec<_>>().join(" ");
    assert!(clean_line(&long).ends_with("..."));
}

/// [fix/review-app] A diary asked for while a meeting runs to the mayor's
/// vote is not dropped by the meeting's driver: `run` hands it back.
#[test]
fn a_meeting_run_hands_back_other_deliveries() {
    let mut w = grown();
    let id = w.citizens.iter().filter(|(_, c)| c.memories.len() >= 2).map(|(id, _)| id).next().expect("someone remembers something");
    let mut v = Voices::default();
    let mut c = Courier::mock();
    v.ask_diary(&w, id, &mut c, false);
    let mut d = Driver::default();
    start_meeting(&mut w, true).expect("a meeting starts");
    let others = d.run(&mut w, &mut c, 400);
    assert_eq!(others.len(), 1);
    assert!(v.take(&others[0]));
    assert!(!v.diary(id).unwrap().pending, "the diary arrived");
}

// ── [ask-committee] the mayor's questions ──────────────────────────────────

use super::questions::{parse_answer, Asker};

fn reply(raw: &str) -> TurnReply {
    TurnReply::from_raw(raw, true)
}

#[test]
fn a_question_reply_says_whether_they_answer() {
    assert_eq!(parse_answer(&reply(r#"{"respond": false, "text": ""}"#)), (false, String::new()));
    assert!(!parse_answer(&reply(r#"{"respond": false, "text": "I would rather not."}"#)).0, "a decline with words is still a decline");
    assert_eq!(parse_answer(&reply(r#"Sure: {"respond": true, "text": "Rents follow the shortage."}"#)), (true, "Rents follow the shortage.".to_string()));
    assert_eq!(parse_answer(&reply(r#"{"respond": "yes", "text": "Build homes."}"#)), (true, "Build homes.".to_string()));
    assert!(!parse_answer(&reply(r#"{"respond": true, "text": "  "}"#)).0, "an empty answer is no answer");
    // Malformed: cut off by the token cap, prose, or nothing usable.
    assert_eq!(parse_answer(&reply(r#"{"respond": true, "text": "The buses are full and"#)), (true, "The buses are full and".to_string()));
    assert!(!parse_answer(&reply(r#"{"respond":false,"text":"#)).0);
    assert_eq!(parse_answer(&reply("We need more depots, mayor.")), (true, "We need more depots, mayor.".to_string()));
    assert!(!parse_answer(&reply("{")).0);
    assert!(!parse_answer(&reply("")).0);
}

#[test]
fn a_question_turn_reads_the_persona_the_report_and_the_thread() {
    let mut w = grown();
    w.committee_ensure();
    let qid = w.committee.ask("  Why   are rents so high? ", w.committee_today()).unwrap();
    let q = w.committee.question(qid).unwrap().clone();
    assert_eq!(q.text, "Why are rents so high?");
    assert_eq!(q.asked.len(), w.committee.seated_ids().len());
    let first = q.asked[0];
    w.committee.answer_question(qid, first, true, "Because nobody builds.", false);
    let q = w.committee.question(qid).unwrap().clone();
    let report = nd_sim::committee::CityReport::build(&w);
    let req = questions::request(&w, &report, &q, q.asked[1]).unwrap();
    assert!(req.task.wants_json());
    assert_eq!(req.task.key(), "question");
    let sys = system_prompt(&req);
    let user = user_prompt(&req);
    let me = req.speaker.as_ref().unwrap();
    assert!(sys.contains("question time") && sys.contains(&me.name) && sys.contains("need not answer"), "{sys}");
    assert!(user.contains("CITY REPORT") && user.contains("Why are rents so high?"), "{user}");
    assert!(user.contains("Because nobody builds.") && user.contains(&w.committee.name_of(first)), "earlier replies are in the thread: {user}");
    assert!(user.contains(r#"{"respond": true"#) && user.contains(r#"{"respond": false"#), "{user}");
    assert!(req.max_tokens <= 400);
}

#[test]
fn the_stand_in_answers_questions_in_character_and_sometimes_declines() {
    let run = || {
        let mut w = grown();
        w.committee_ensure();
        let mut a = Asker::default();
        let mut c = Courier::mock();
        let today = w.committee_today();
        let qs = ["Why are rents so high?", "Is crime getting worse?", "What do you all think of the new year?", "Should we cut taxes?", "How are the buses doing?"];
        for t in qs {
            w.committee.ask(t, today).unwrap();
        }
        let others = a.run(&mut w, &mut c, 200);
        assert!(others.is_empty());
        w.committee.questions.clone()
    };
    let qs = run();
    assert_eq!(qs, run(), "deterministic");
    let mut declined = 0;
    for q in &qs {
        assert!(q.is_complete(), "every member asked has had their turn: {q:?}");
        assert_eq!(q.replies.len(), q.asked.len(), "one reply per member");
        assert!(q.answers().count() >= 1, "somebody answers: {q:?}");
        declined += q.replies.iter().filter(|r| !r.respond).count();
        for r in q.answers() {
            assert!(!r.text.is_empty() && !r.stand_in);
        }
    }
    assert!(declined > 0, "some members chose not to answer");
    let rents = &qs[0];
    let housing = rents.replies.iter().find(|r| r.scope == "Housing").expect("a housing member is seated");
    assert!(housing.respond, "the housing member answers a question on rents: {rents:?}");
    for q in &qs {
        println!("Q: {}", q.text);
        for r in &q.replies {
            println!("  {} ({}): {}", r.name, if r.respond { "answers" } else { "no comment" }, r.text);
        }
    }
}

#[test]
fn questions_survive_a_save_and_old_saves_load_without_them() {
    let mut w = grown();
    w.committee_ensure();
    let mut a = Asker::default();
    let mut c = Courier::mock();
    w.committee.ask("Why are rents so high?", 0).unwrap();
    let _ = a.run(&mut w, &mut c, 100);
    // A second question still being answered when the city is saved.
    let pending = w.committee.ask("Is crime getting worse?", 0).unwrap();
    let text = w.to_ron().unwrap();
    let mut back = World::from_ron(&text).unwrap();
    assert_eq!(back.committee.questions, w.committee.questions);
    assert!(!back.committee.question(pending).unwrap().is_complete());
    // After a load the asker picks up where the thread stood.
    let mut a2 = Asker::default();
    let _ = a2.run(&mut back, &mut c, 100);
    assert!(back.committee.question(pending).unwrap().is_complete());
    // A save from before the feature: no `questions` field anywhere.
    let mut fresh = grown();
    fresh.committee_ensure();
    let text = fresh.to_ron().unwrap();
    let old: String = text.lines().filter(|l| !l.trim_start().starts_with("questions:")).collect::<Vec<_>>().join("\n");
    assert!(!old.contains("questions:"), "the field was cut");
    let back = World::from_ron(&old).unwrap();
    assert!(back.committee.questions.is_empty());
    assert_eq!(back.committee.pool, fresh.committee.pool);
}

thread_local!(static HEARD: std::cell::RefCell<Vec<TurnRequest>> = const { std::cell::RefCell::new(Vec::new()) });

/// The stand-in, keeping a copy of every request.
#[derive(Default)]
struct Recorder(MockCommittee);

impl CommitteeBackend for Recorder {
    fn submit(&mut self, id: u64, req: TurnRequest) {
        HEARD.with(|h| h.borrow_mut().push(req.clone()));
        self.0.submit(id, req);
    }
    fn poll(&mut self) -> Vec<(u64, Result<TurnReply, String>)> {
        self.0.poll()
    }
    fn name(&self) -> String {
        "recorder".into()
    }
    fn is_mock(&self) -> bool {
        true
    }
}

#[test]
fn the_next_meeting_reads_the_questions_and_marks_them_discussed() {
    let mut w = grown();
    w.committee_ensure();
    let mut a = Asker::default();
    let mut c = Courier::new(Box::new(Recorder::default()), Duration::from_secs(60));
    let today = w.committee_today();
    let done = w.committee.ask("Why are rents so high?", today).unwrap();
    let _ = a.run(&mut w, &mut c, 100);
    let said = w.committee.question(done).unwrap().answers().next().unwrap().text.clone();
    // One still waiting when the meeting opens: it stays for the next.
    let waiting = w.committee.ask("Is crime getting worse?", today).unwrap();
    HEARD.with(|h| h.borrow_mut().clear());
    let mut d = Driver::default();
    let number = start_meeting(&mut w, true).unwrap();
    let m = w.committee.meeting.as_ref().unwrap();
    assert!(m.questions.contains("Why are rents so high?"), "{}", m.questions);
    let head: String = said.chars().take(40).collect();
    assert!(m.questions.contains(&head), "the digest has what they said: {}", m.questions);
    assert!(!m.questions.contains("Is crime getting worse?"), "a thread still being answered waits");
    assert_eq!(m.questions_read, vec![done]);
    assert!(m.questions.chars().count() <= nd_sim::committee::questions::DIGEST_MAX_CHARS + 200);
    assert_eq!(w.committee.question(done).unwrap().discussed, Some(number));
    assert_eq!(w.committee.question(waiting).unwrap().discussed, None);
    let _ = d.run(&mut w, &mut c, 400);
    let heard = HEARD.with(|h| h.borrow().clone());
    let status: Vec<&TurnRequest> = heard.iter().filter(|r| matches!(r.task, Task::Status { .. })).collect();
    assert!(!status.is_empty());
    for r in &status {
        assert!(user_prompt(r).contains("Questions the mayor put to the committee"), "every member's turn has the digest");
    }
    let m = w.committee.meeting.as_ref().unwrap();
    assert!(m.transcript.iter().any(|l| l.kind == LineKind::Note && l.text.contains("question")), "the secretary notes the questions");
    abstain_rest(&mut w).unwrap();
    let _ = d.run(&mut w, &mut c, 400);
    assert!(w.committee.meeting.is_none());
    // The late thread is answered and waits for the meeting after.
    let _ = a.run(&mut w, &mut c, 100);
    assert_eq!(w.committee.undiscussed().len(), 1);
    let digest = w.committee.questions_digest();
    assert!(digest.contains("Is crime getting worse?") && !digest.contains("Why are rents so high?"), "{digest}");
    // Discussed threads stay in the log.
    assert_eq!(w.committee.questions.len(), 2);
}

#[test]
fn the_question_log_is_bounded() {
    let mut w = grown();
    w.committee_ensure();
    for i in 0..(nd_sim::committee::questions::QUESTIONS_KEPT + 7) {
        w.committee.ask(&format!("Question {i}?"), 0).unwrap();
    }
    assert_eq!(w.committee.questions.len(), nd_sim::committee::questions::QUESTIONS_KEPT);
    assert!(w.committee.ask("   ", 0).is_err());
    let long = "word ".repeat(400);
    let id = w.committee.ask(&long, 0).unwrap();
    assert!(w.committee.question(id).unwrap().text.chars().count() <= nd_sim::committee::questions::QUESTION_MAX_CHARS + 3);
}
