//! Remote control: a localhost HTTP JSON API for driving and inspecting the
//! running game from scripts and from the MCP server in `tools/mcp`.
//! Protocol and commands are documented in `docs/REMOTE.md`.
//!
//! Why this shape: the sim, the tools and the camera are all Bevy resources,
//! so commands have to run on the main thread. A `tiny_http` thread accepts
//! requests and parks each one, with a reply channel, in an mpsc queue;
//! `remote_commands` drains the queue once per frame in `SimSet::Input`,
//! before the player's tools, runs each command against the ECS and sends
//! the JSON reply back. The HTTP thread waits on that channel (bounded by a
//! timeout) and answers. Nothing here blocks the frame: the drain is
//! non-blocking and no lock is held across frames.
//!
//! `ND_REMOTE=0` disables the server; `ND_REMOTE_PORT` picks the port
//! (default 4817). It binds 127.0.0.1 only: the API has no authentication.

/// [concerns] complaints by category and city happiness.
/// [committee] `committee`, `committee_meeting`.
mod committee;
mod concerns;
/// [districts] [tourism] `district` and `tourism`.
mod districts;
/// [directory] `directory`: City Data's Directory rows, sorted and filtered.
mod directory;
/// [economy] business pressure: what the city's businesses want
mod economy;
/// [camera] `follow`, `zoom_to`, `gift`; delivery robots' detail.
mod follow;
/// Request checks (Host, Origin, content type, body size) and the folders
/// file commands may use.
mod grounds;
mod guard;
mod history;
/// [identity] `city_name` and `game_history`.
mod identity;
mod junction;
mod land;
mod list;
mod map;
// [events] street races, raves, road condition
mod nights;
/// [places] taste, favourite places, venue ratings.
mod places;
/// [land] Owners, prices, merged plots, pressure and the super-elite.
mod property;
mod report;
/// [society] crime, homelessness and trash.
mod society;
/// [social] Friends, feuds, groups.
mod social;
/// [subway] The `subway` command and report section.
mod subway;
mod taxes;
/// [toll-inspect] A toll booth in full for `inspect {toll}`.
mod tolls;
/// [road-inspect] A street piece in full for `inspect {road}`.
mod road;
// [supertall] the skyscraper league
mod skyline;
/// [taxis] Taxi companies and cargo hubs in the report and inspector.
mod taxis;
mod trains;
// [fire-scene] Emergency units: inspect, select, list.
mod units;
mod view;
mod window;
/// [ferry-routes] A dock's routes and `inspect {ship}`.
mod ferry;

use std::collections::VecDeque;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use nd_sim::roads::RoadKind;
use nd_sim::zoning::point_in_polygon;
use nd_sim::{Gesture, GestureId, LotId, RailFacility, SimEvent, Vec2 as SVec2};
use serde_json::{json, Value};

use crate::assets::GameAssets;
use crate::camera::{CameraMode, OrbitCamera, WalkState};
use crate::render::{Lighting, Overlays, RenderStats};
use nd_sim::survey::LandLayer;
use crate::sim::{Sim, SimSet, SPEEDS};
use crate::tools::{marquee_rect, EffectivePlan, Selection, Tool, ToolState, TransitMode, ZoneMode, BRUSH_MAX, BRUSH_MIN, ROAD_PRESETS};
use crate::ui::{load_now, save_now, Saves, UiFlags};
use history::History;
use view::*;
use window::WindowRequests;

const DEFAULT_PORT: u16 = 4817;
/// How long the HTTP thread waits for the main thread to answer. A frame
/// normally takes milliseconds; a long `step` or a stalled window can take
/// longer, and the caller gets a 504 rather than hanging forever.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for a command the main thread has already started
/// (a `step` of many days can run for a minute).
const LONG_REPLY_TIMEOUT: Duration = Duration::from_secs(300);
/// Events kept for `events {since}`; at 256× a busy city makes hundreds a
/// second, so a poll every few seconds still sees everything.
const EVENT_RING: usize = 2000;
/// Upper bound for `step`, so a typo cannot freeze the game for minutes.
const MAX_STEP: u64 = 100_000;

pub struct RemotePlugin;

impl Plugin for RemotePlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("ND_REMOTE").is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("off")) {
            info!("remote API disabled (ND_REMOTE=0)");
            return;
        }
        let port = std::env::var("ND_REMOTE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
        let (tx, rx) = mpsc::channel::<Pending>();
        match tiny_http::Server::http(("127.0.0.1", port)) {
            Ok(server) => {
                info!("remote API listening on http://127.0.0.1:{port}  (POST /cmd, GET /status)");
                std::thread::Builder::new().name("nd-remote-http".into()).spawn(move || serve(server, tx, port)).expect("spawn remote http thread");
            }
            Err(e) => {
                warn!("remote API could not bind 127.0.0.1:{port}: {e}; set ND_REMOTE_PORT or ND_REMOTE=0");
                return;
            }
        }
        app.insert_resource(Inbox(Mutex::new(rx)))
            .init_resource::<EventLog>()
            .init_resource::<History>()
            .init_resource::<WindowRequests>()
            .add_systems(Update, remote_commands.in_set(SimSet::Input).before(crate::tools::tool_input))
            .add_systems(Update, (log_events, record_history).in_set(SimSet::Sync))
            // Before the panel draws, so a request shows the same frame.
            .add_systems(bevy_egui::EguiPrimaryContextPass, window::apply_window_requests.before(crate::ui::side_panel));
    }
}

/// One parked request: the parsed command, where the reply goes, and
/// whether it is still wanted ([`QUEUED`] until the main thread takes it,
/// [`CANCELLED`] if the HTTP side gave up first).
struct Pending {
    cmd: Value,
    reply: Sender<Reply>,
    state: Arc<AtomicU8>,
}

const QUEUED: u8 = 0;
const TAKEN: u8 = 1;
const CANCELLED: u8 = 2;

struct Reply {
    status: u16,
    body: Value,
}

/// `Receiver` is `Send` but not `Sync`; a resource must be both, so the
/// mutex makes the compiler happy. Only `remote_commands` ever locks it,
/// and only for the length of a non-blocking drain.
#[derive(Resource)]
struct Inbox(Mutex<Receiver<Pending>>);

/// Numbered events for `events {since}`: `SimEvent`s plus status-line
/// changes, each with the tick it happened at.
#[derive(Resource, Default)]
struct EventLog {
    seq: u64,
    ring: VecDeque<Value>,
    last_message: String,
    last_status: String,
}

impl EventLog {
    fn push(&mut self, tick: u64, mut v: Value) {
        self.seq += 1;
        v["seq"] = json!(self.seq);
        v["tick"] = json!(tick);
        self.ring.push_back(v);
        if self.ring.len() > EVENT_RING {
            self.ring.pop_front();
        }
    }
}


// ── HTTP thread ─────────────────────────────────────────────────────────────

fn serve(server: tiny_http::Server, tx: Sender<Pending>, port: u16) {
    let in_flight = Arc::new(AtomicUsize::new(0));
    for req in server.incoming_requests() {
        // One thread per request, so a client that trickles its body holds
        // up only itself; capped, so a flood is answered 503 at once.
        if in_flight.fetch_add(1, Ordering::SeqCst) >= guard::MAX_IN_FLIGHT {
            in_flight.fetch_sub(1, Ordering::SeqCst);
            respond(req, Reply { status: 503, body: json!({ "ok": false, "error": "too many requests at once; retry" }) });
            continue;
        }
        let tx = tx.clone();
        let count = in_flight.clone();
        let spawned = std::thread::Builder::new().name("nd-remote-req".into()).spawn(move || {
            handle(req, &tx, port);
            count.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            in_flight.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn respond(req: tiny_http::Request, reply: Reply) {
    let text = reply.body.to_string();
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).expect("static header");
    let _ = req.respond(tiny_http::Response::from_string(text).with_status_code(reply.status).with_header(header));
}

/// One request, on its own thread: check who is asking (before reading the
/// body), read a bounded body, queue the command and wait for its reply.
fn handle(mut req: tiny_http::Request, tx: &Sender<Pending>, port: u16) {
    // Owned copies: reading the body borrows the request mutably.
    let method = req.method().as_str().to_string();
    let url = req.url().split('?').next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> = req.headers().iter().map(|h| (h.field.as_str().as_str().to_string(), h.value.as_str().to_string())).collect();
    if let Err((status, error)) = guard::check_headers(&method, &headers, port) {
        return respond(req, Reply { status, body: json!({ "ok": false, "error": error }) });
    }
    let cmd = match (method.as_str(), url.as_str()) {
        ("GET", "/status") | ("GET", "/status/") => Ok(json!({ "cmd": "status" })),
        ("GET", "/help") => Ok(json!({ "cmd": "help" })),
        ("POST", "/cmd") | ("POST", "/cmd/") | ("POST", "/") => read_body(&mut req),
        _ => Err((404, "use POST /cmd with a JSON body {\"cmd\": ...}, or GET /status".to_string())),
    };
    let reply = match cmd {
        Err((status, error)) => Reply { status, body: json!({ "ok": false, "error": error }) },
        Ok(cmd) => {
            let (reply_tx, reply_rx) = mpsc::channel();
            let state = Arc::new(AtomicU8::new(QUEUED));
            if tx.send(Pending { cmd, reply: reply_tx, state: state.clone() }).is_err() {
                // The app is shutting down; the receiver is gone.
                return;
            }
            match reply_rx.recv_timeout(REPLY_TIMEOUT) {
                Ok(r) => r,
                // Not started yet: withdraw it, so the caller can retry
                // without the command running twice.
                Err(_) if state.compare_exchange(QUEUED, CANCELLED, Ordering::SeqCst, Ordering::SeqCst).is_ok() => {
                    Reply { status: 504, body: json!({ "ok": false, "error": "the game did not answer in time (frame stalled or window closing); the command was withdrawn and did not run" }) }
                }
                // Already running (a long `step`): its answer is coming.
                Err(_) => match reply_rx.recv_timeout(LONG_REPLY_TIMEOUT) {
                    Ok(r) => r,
                    Err(_) => Reply { status: 504, body: json!({ "ok": false, "error": "the command started but did not finish in time; it may still take effect" }) },
                },
            }
        }
    };
    respond(req, reply);
}

/// The body as JSON, at most [`guard::MAX_BODY`] bytes.
fn read_body(req: &mut tiny_http::Request) -> Result<Value, (u16, String)> {
    let too_big = || (413, format!("body larger than {} bytes", guard::MAX_BODY));
    if req.body_length().is_some_and(|n| n > guard::MAX_BODY) {
        return Err(too_big());
    }
    let mut body = Vec::new();
    req.as_reader().take(guard::MAX_BODY as u64 + 1).read_to_end(&mut body).map_err(|e| (400, format!("could not read body: {e}")))?;
    if body.len() > guard::MAX_BODY {
        return Err(too_big());
    }
    serde_json::from_slice::<Value>(&body).map_err(|e| (400, format!("body is not JSON: {e}")))
}

// ── main-thread systems ─────────────────────────────────────────────────────

/// Everything a command may read or write, in one bundle so the dispatcher
/// has a single argument.
#[derive(SystemParam)]
struct Ctx<'w, 's> {
    sim: ResMut<'w, Sim>,
    tools: ResMut<'w, ToolState>,
    selection: ResMut<'w, Selection>,
    eplan: ResMut<'w, EffectivePlan>,
    flags: ResMut<'w, UiFlags>,
    saves: ResMut<'w, Saves>,
    overlays: ResMut<'w, Overlays>,
    lighting: ResMut<'w, Lighting>,
    mode: ResMut<'w, CameraMode>,
    walk: ResMut<'w, WalkState>,
    /// [camera] The camera's follow (shared with the inspector's button)
    /// and its smooth moves.
    follow: ResMut<'w, crate::camera::CameraFollow>,
    fly: ResMut<'w, crate::camera::CameraFly>,
    log: ResMut<'w, EventLog>,
    history: ResMut<'w, History>,
    windows: ResMut<'w, WindowRequests>,
    cams: Query<'w, 's, &'static mut OrbitCamera>,
    stats: Res<'w, RenderStats>,
    assets: Res<'w, GameAssets>,
    diagnostics: Res<'w, DiagnosticsStore>,
    commands: Commands<'w, 's>,
    claude: ResMut<'w, crate::flags::ClaudeFlags>,
    /// [committee] The committee's backend and meeting driver.
    committee: ResMut<'w, crate::committee::CommitteeRt>,
    /// [build-highlight] What the Build window's section shows on the map.
    highlight: Res<'w, crate::build_highlight::BuildHighlight>,
}

fn remote_commands(inbox: Res<Inbox>, mut ctx: Ctx) {
    // Take the batch out first: the lock must not be held while commands run.
    let batch: Vec<Pending> = match inbox.0.lock() {
        Ok(rx) => rx.try_iter().collect(),
        Err(_) => return,
    };
    for p in batch {
        // Withdrawn after a 504: running it now would surprise a client
        // that was told it did not run.
        if p.state.compare_exchange(QUEUED, TAKEN, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            continue;
        }
        let reply = match execute(&p.cmd, &mut ctx) {
            Ok(mut body) => {
                if body.get("ok").is_none() {
                    body["ok"] = json!(true);
                }
                Reply { status: 200, body }
            }
            Err(e) => Reply { status: 400, body: json!({ "ok": false, "error": e }) },
        };
        // The HTTP side may have timed out and gone; nothing to do then.
        let _ = p.reply.send(reply);
    }
}

/// Record this frame's `SimEvent`s and status-line changes. Runs in `Sync`,
/// where `Sim::events` holds the frame's events for every reader.
fn log_events(sim: Res<Sim>, flags: Res<UiFlags>, tools: Res<ToolState>, mut log: ResMut<EventLog>) {
    let w = &sim.world;
    let tick = w.clock.tick;
    for e in &sim.events {
        let v = match e {
            SimEvent::RoadsChanged => json!({ "kind": "RoadsChanged" }),
            SimEvent::LotsChanged => json!({ "kind": "LotsChanged" }),
            SimEvent::PlanChanged => json!({ "kind": "PlanChanged" }),
            SimEvent::NewDay(d) => json!({ "kind": "NewDay", "day": d + 1 }),
            SimEvent::BuildingBuilt(b) => json!({ "kind": "BuildingBuilt", "building": key_str(*b), "name": building_name(w, Some(*b)) }),
            SimEvent::BuildingChanged(b) => json!({ "kind": "BuildingChanged", "building": key_str(*b), "name": building_name(w, Some(*b)) }),
            SimEvent::BuildingRemoved(b, l) => json!({ "kind": "BuildingRemoved", "building": key_str(*b), "lot": key_str(*l) }),
            SimEvent::Expropriated(b, l) => json!({ "kind": "Expropriated", "building": key_str(*b), "lot": key_str(*l) }),
            SimEvent::BusinessClosed(b) => json!({ "kind": "BusinessClosed", "building": key_str(*b), "name": building_name(w, Some(*b)) }),
            SimEvent::CitizenArrived(c) => json!({ "kind": "CitizenArrived", "citizen": key_str(*c), "name": w.citizens.get(*c).map(|x| x.name.as_str()) }),
            SimEvent::CitizenLeft(c) => json!({ "kind": "CitizenLeft", "citizen": key_str(*c) }),
            SimEvent::IncidentStarted(i) | SimEvent::EmergencyCall(i) | SimEvent::UnitDispatched(i) | SimEvent::UnitOnScene(i) | SimEvent::IncidentResolved(i) => {
                let kind = match e {
                    SimEvent::IncidentStarted(_) => "IncidentStarted",
                    SimEvent::EmergencyCall(_) => "EmergencyCall",
                    SimEvent::UnitDispatched(_) => "UnitDispatched",
                    SimEvent::UnitOnScene(_) => "UnitOnScene",
                    _ => "IncidentResolved",
                };
                match w.services.incident(*i) {
                    Some(inc) => json!({ "kind": kind, "incident": i, "type": inc.kind.label(), "building": key_str(inc.building), "name": building_name(w, Some(inc.building)), "outcome": inc.outcome.map(|o| o.label()) }),
                    None => json!({ "kind": kind, "incident": i }),
                }
            }
            SimEvent::FireSpread(b) => json!({ "kind": "FireSpread", "building": key_str(*b), "name": building_name(w, Some(*b)) }),
            SimEvent::BurnedDown(b, l) => json!({ "kind": "BurnedDown", "building": key_str(*b), "lot": key_str(*l) }),
            SimEvent::Arrested(i, c) => json!({ "kind": "Arrested", "incident": i, "citizen": key_str(*c), "name": w.citizens.get(*c).map(|x| x.name.as_str()) }),
            SimEvent::Released(c) => json!({ "kind": "Released", "citizen": key_str(*c), "name": w.citizens.get(*c).map(|x| x.name.as_str()) }),
            SimEvent::Crash(id) => {
                let r = w.driving.recent.iter().find(|r| r.id == *id);
                json!({ "kind": "Crash", "crash": id, "pos": r.map(|r| [r.pos.x, r.pos.y]), "cause": r.map(|r| r.cause.label()), "severity": r.map(|r| r.severity), "injured": r.map(|r| r.injured.len()), "place": r.map(|r| r.place.clone()), "incident": r.and_then(|r| r.incident) })
            }
            SimEvent::Takeover(id) => json!({ "kind": "Takeover", "takeover": id, "place": w.takeover().map(|t| t.place.clone()) }),
            SimEvent::TakeoverOver(id) => json!({ "kind": "TakeoverOver", "takeover": id }),
            SimEvent::News(seq) => {
                let n = w.street_events.news_item(*seq);
                json!({ "kind": "News", "seq": seq, "event": n.map(|n| n.kind.label()), "headline": n.map(|n| n.headline.as_str()), "pos": n.map(|n| [n.pos.x, n.pos.y]) })
            }
            SimEvent::PathTrimmed { gesture, removed, grounds } => json!({ "kind": "PathTrimmed", "gesture_id": gesture.0, "removed": removed, "grounds": grounds.key() }),
            SimEvent::StadiumEvent(b) => json!({ "kind": "StadiumEvent", "building": key_str(*b), "name": building_name(w, Some(*b)) }),
            SimEvent::NewThought(c) => {
                let cit = w.citizens.get(*c);
                json!({ "kind": "NewThought", "citizen": key_str(*c), "name": cit.map(|x| x.name.as_str()), "thought": cit.map(|x| x.thought.as_str()) })
            }
        };
        log.push(tick, v);
    }
    if flags.message != log.last_message {
        log.last_message = flags.message.clone();
        let text = log.last_message.clone();
        log.push(tick, json!({ "kind": "Message", "text": text }));
    }
    if tools.status != log.last_status {
        log.last_status = tools.status.clone();
        let text = log.last_status.clone();
        log.push(tick, json!({ "kind": "ToolStatus", "text": text }));
    }
}

/// Sample the city into the history rings when an hour or a day turns
/// (cheap otherwise: a few integer compares and four counters copied).
fn record_history(sim: Res<Sim>, mut history: ResMut<History>) {
    history.follow_city(sim.generation, &sim.world, "city replaced");
    history.observe(&sim.world);
}


// ── dispatch ────────────────────────────────────────────────────────────────

fn execute(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let name = cmd.get("cmd").and_then(Value::as_str).ok_or("missing \"cmd\" field; try {\"cmd\": \"help\"}")?;
    match name {
        "help" => Ok(help()),
        "status" => Ok(status(ctx)),
        "pause" => {
            let paused = opt_bool(cmd, "paused")?.unwrap_or(!ctx.sim.paused);
            ctx.sim.paused = paused;
            Ok(json!({ "paused": paused }))
        }
        "speed" => {
            let sim = &mut ctx.sim;
            if let Some(i) = opt_u64(cmd, "index")? {
                if i as usize >= SPEEDS.len() {
                    return Err(format!("speed index must be 0..{}", SPEEDS.len() - 1));
                }
                sim.speed_index = i as usize;
            } else if let Some(v) = opt_f32(cmd, "value")? {
                // The nearest step on a log scale: 10 picks 16×, 1.5 picks 1×.
                let v = v.max(0.01).ln();
                sim.speed_index = SPEEDS.iter().enumerate().min_by(|a, b| ((a.1).1.ln() - v).abs().total_cmp(&((b.1).1.ln() - v).abs())).map_or(1, |(i, _)| i);
            } else {
                return Err("speed needs {index: 0..5} or {value: multiple of real time}".into());
            }
            Ok(json!({ "speed_index": sim.speed_index, "speed": SPEEDS[sim.speed_index].1, "ticks_per_second": sim.speed(), "label": SPEEDS[sim.speed_index].0 }))
        }
        "step" => {
            let n = opt_u64(cmd, "ticks")?.unwrap_or(1);
            if n > MAX_STEP {
                return Err(format!("ticks must be at most {MAX_STEP}"));
            }
            let start = std::time::Instant::now();
            // Hour by hour, each chunk ending at minute 59, so history sees
            // every hour and reads the day's counters before midnight
            // resets them, however many days one request covers.
            let mut left = n;
            while left > 0 {
                let r = ctx.sim.world.clock.tick % 60;
                let chunk = left.min(if r < 59 { 59 - r } else { 60 });
                ctx.sim.step_ticks(chunk as u32);
                ctx.history.observe(&ctx.sim.world);
                left -= chunk;
            }
            Ok(json!({ "ticks": n, "clock": clock(&ctx.sim), "took_ms": start.elapsed().as_millis() as u64 }))
        }
        "seed_demo" => {
            ctx.saves.title = false;
            ctx.sim.seed_demo_district();
            ctx.tools.status = "Demo district laid out around the centre".into();
            Ok(json!({ "buildings": ctx.sim.world.buildings.len(), "lots": ctx.sim.world.lots.len() }))
        }
        "save" => save(cmd, ctx),
        "city_name" | "city_identity" => identity::city_name(cmd, ctx),
        "game_history" => identity::game_history(cmd, ctx),
        "load" => {
            let path = allowed_path(req_str(cmd, "path")?, "ron", false)?;
            let Ctx { sim, tools, selection, eplan, flags, saves, follow, fly, .. } = &mut *ctx;
            load_now(&path, sim, tools, selection, eplan, flags);
            saves.mark_dirty();
            saves.title = false;
            follow.stop();
            fly.flight = None;
            if flags.message.starts_with("load failed") {
                return Err(flags.message.clone());
            }
            ctx.history.follow_city(ctx.sim.generation, &ctx.sim.world, "city loaded");
            Ok(json!({ "message": ctx.flags.message, "clock": clock(&ctx.sim), "paused": ctx.sim.paused }))
        }
        "new_game" => {
            let params = land::params_from(cmd)?;
            ctx.saves.title = false;
            ctx.sim.paused = false;
            ctx.sim.new_game(params);
            if let Some(name) = opt_str(cmd, "name")? {
                ctx.sim.set_city_name(name);
            }
            *ctx.tools = ToolState::default();
            *ctx.selection = Selection::None;
            ctx.eplan.invalidate();
            ctx.follow.stop();
            ctx.fly.flight = None;
            ctx.tools.status = format!("New city: {} {} landscape", ctx.sim.world.landscape.params.climate.label(), ctx.sim.world.landscape.params.preset.label());
            ctx.history.follow_city(ctx.sim.generation, &ctx.sim.world, "new game");
            let mut v = land::summary(&ctx.sim.world);
            v["identity"] = identity::identity_json(&ctx.sim.world);
            Ok(v)
        }
        "generate_city" => {
            let (preset, params) = land::city_params_from(cmd)?;
            ctx.saves.title = false;
            ctx.sim.paused = false;
            let start = std::time::Instant::now();
            let (plan, laid) = ctx.sim.generate_city(preset, params);
            if let Some(name) = opt_str(cmd, "name")? {
                ctx.sim.set_city_name(name);
            }
            *ctx.tools = ToolState::default();
            *ctx.selection = Selection::None;
            ctx.eplan.invalidate();
            ctx.follow.stop();
            ctx.fly.flight = None;
            ctx.tools.status = format!("New city: {} on a {} landscape", preset.label(), ctx.sim.world.landscape.params.preset.label());
            ctx.history.follow_city(ctx.sim.generation, &ctx.sim.world, "generated city");
            let mut v = land::summary(&ctx.sim.world);
            let class = |c: nd_procgen::citygen::RoadClass| json!({ "roads": plan.roads.iter().filter(|r| r.class == c).count(), "length_m": round(plan.road_length(c)) });
            v["city"] = json!({
                "preset": preset.key(),
                "seed": plan.seed,
                "arterial": class(nd_procgen::citygen::RoadClass::Arterial),
                "collector": class(nd_procgen::citygen::RoadClass::Collector),
                "local": class(nd_procgen::citygen::RoadClass::Local),
                "zones": laid.zones,
                "lots": laid.lots,
                "interchanges": laid.links,
                "way_in": laid.way_in,
                "centres": plan.cores.iter().map(|c| v2(*c)).collect::<Vec<_>>(),
                "took_ms": start.elapsed().as_millis() as u64,
            });
            v["identity"] = identity::identity_json(&ctx.sim.world);
            Ok(v)
        }
        "terrain_at" => Ok(land::terrain_at(&ctx.sim.world, req_point(cmd)?)),
        // [dirty-industry] Pay to clean up poisoned ground: {x, y, radius}.
        "remediate" => {
            let p = req_point(cmd)?;
            let r = opt_f32(cmd, "radius")?.unwrap_or(100.0);
            let (cost, removed) = ctx.sim.world.remediate(p, r)?;
            Ok(json!({ "ok": true, "cost": round(cost), "removed_cells": round(removed), "treasury": round(ctx.sim.world.economy.treasury) }))
        }
        // [dirty-industry] The Bulldoze tool's clean-up: a crew digs it out
        // over a few days (`quote: true` only prices it).
        "cleanup" | "clean_up" => {
            let p = req_point(cmd)?;
            let r = opt_f32(cmd, "radius")?.unwrap_or(100.0);
            let w = &mut ctx.sim.world;
            let q = if opt_bool(cmd, "quote")?.unwrap_or(false) { w.cleanup_quote(p, r)? } else { w.start_cleanup(p, r)? };
            let crews: Vec<_> = w.cleanups.iter().map(|j| json!({ "id": j.id, "at": [round(j.center.x), round(j.center.y)], "radius": round(j.radius), "progress": round(j.progress()), "days": j.hours / 24, "cost": round(j.cost) })).collect();
            Ok(json!({ "ok": true, "radius": round(q.radius), "cells": q.cells, "amount": round(q.amount), "cost": round(q.cost), "days": q.days, "treasury": round(w.economy.treasury), "crews": crews }))
        }
        // [dirty-industry] Testing: pour contamination onto the ground.
        "contaminate" => {
            let p = req_point(cmd)?;
            let r = opt_f32(cmd, "radius")?.unwrap_or(60.0).clamp(0.0, 400.0);
            let a = opt_f32(cmd, "amount")?.unwrap_or(0.6).clamp(0.0, 1.0);
            ctx.sim.world.contamination.add(p, r, a);
            Ok(json!({ "ok": true, "at": [p.x, p.y], "contamination": round(ctx.sim.world.contamination.at(p)) }))
        }
        // [social] Testing: a street-art crew's mural on a building now.
        "paint_mural" => {
            let bid: nd_sim::BuildingId = key_from(field(cmd, "building").ok_or("missing building")?)?;
            let w = &mut ctx.sim.world;
            let b = w.buildings.get(bid).ok_or("no such building")?;
            let lot = b.lot;
            let l = w.lots.get(lot).ok_or("no lot")?;
            let along = (l.front_edge[1] - l.front_edge[0]).normalize_or_zero();
            let mid = (l.front_edge[0] + l.front_edge[1]) * 0.5;
            let inward = (l.center - mid).normalize_or_zero();
            let group = match opt_u64(cmd, "group")? {
                Some(g) => g as u32,
                None => w.social.groups.iter().find(|g| g.kind == nd_sim::social::GroupKind::StreetArt).map_or(0, |g| g.id),
            };
            let quality = opt_f32(cmd, "quality")?.unwrap_or(0.6).clamp(-1.0, 1.0);
            let seed = opt_u64(cmd, "seed")?.unwrap_or((l.center.x * 31.0 + l.center.y * 17.0).abs() as u64 ^ 0x5EED);
            w.social.murals.retain(|m| m.lot != lot);
            w.social.murals.push(nd_sim::social::Mural { group, lot, building: Some(bid), seed, day: w.clock.day(), quality, pos: mid + inward * 0.9, along, facing: -inward, width: 6.0, height: 3.4 });
            Ok(json!({ "ok": true, "group": group, "at": [mid.x, mid.y], "facing": [-inward.x, -inward.y] }))
        }
        // [dirty-industry] Dirty plants and contamination near a point.
        "dirt_at" | "contamination_at" => {
            let p = req_point(cmd)?;
            let w = &ctx.sim.world;
            Ok(json!({ "at": [p.x, p.y], "contamination": round(w.contamination.at(p)), "dirty_nearby": land::dirty_near(w, p) }))
        }
        // [grid-weather] Wind and sun at a point; y may be given as z (the
        // renderer's name for the sim's y).
        "resource" => {
            let p = match (opt_f32(cmd, "x")?, opt_f32(cmd, "y")?.or(opt_f32(cmd, "z")?)) {
                (Some(x), Some(y)) => SVec2::new(x, y),
                _ => req_point(cmd)?,
            };
            Ok(land::resource_at(&ctx.sim.world, p))
        }
        "grid" => Ok(land::grid_json(&ctx.sim.world)),
        "weather" => Ok(land::weather_json(&ctx.sim.world)),
        "landscape" => Ok(land::summary(&ctx.sim.world)),
        "ships" => Ok(economy::ships(&ctx.sim.world)),
        // [climate] Dev: set the sky, to see rain, snow or sun on demand.
        "set_weather" => {
            let w = &mut ctx.sim.world;
            if let Some(c) = opt_f32(cmd, "cloud")? {
                w.weather.cloud = c.clamp(0.0, 1.0);
            }
            if let Some(v) = opt_f32(cmd, "wind")? {
                w.weather.wind = v.clamp(0.05, 1.0);
            }
            if let Some(t) = opt_f32(cmd, "temp_c")? {
                w.weather.temp_c = t.clamp(-50.0, 55.0);
            }
            Ok(json!({ "cloud": round(w.weather.cloud), "wind": round(w.weather.wind), "temp_c": round(w.weather.temp_c), "spell": format!("{:?}", w.weather.spell).to_lowercase(), "label": w.weather.label(), "climate": w.landscape.params.climate.key(), "weather_climate": w.weather.climate.key(), "precipitation": crate::render::climate::precip_now(w).map(|(k, i)| json!({ "kind": k, "intensity": round(i) })) }))
        }
        "grant" => {
            // Dev: money for trying things out.
            let amount = opt_f32(cmd, "amount")?.unwrap_or(100_000.0);
            ctx.sim.world.economy.treasury += amount;
            Ok(json!({ "treasury": round(ctx.sim.world.economy.treasury) }))
        }
        "walk_to" => {
            // Dev: send a citizen out on foot (to try crossings and footbridges).
            let id = key_from(field(cmd, "citizen").ok_or("missing field \"citizen\": id")?)?;
            let to = opt_vec2(cmd, "to")?.ok_or("missing field \"to\": [x, y]")?;
            let from = opt_vec2(cmd, "from")?;
            let n = ctx.sim.world.dev_walk(id, from, to)?;
            Ok(json!({ "citizen": key_str(id), "route_points": n }))
        }
        "list_saves" => Ok(list_saves()),
        "plan" => Ok(plan(ctx)),
        "add_road" => add_road(cmd, ctx),
        "grounds_junctions" | "park_junctions" => {
            let lot: Option<nd_sim::LotId> = field(cmd, "lot").map(key_from).transpose()?;
            let near = opt_vec2(cmd, "near")?;
            let radius = opt_f32(cmd, "radius")?.unwrap_or(150.0);
            Ok(grounds::list(&ctx.sim.world, lot, opt_str(cmd, "kind")?, near, radius))
        }
        "add_roundabout" => add_roundabout(cmd, ctx),
        // [interchange] A diamond interchange across the highway.
        "add_interchange" | "interchange" => add_interchange(cmd, ctx),
        "add_zone" => add_zone(cmd, ctx),
        "paint" => paint(cmd, ctx),
        "bulldoze" => {
            // [free-placement] A city building goes at once, its ground back
            // to the zoning plan.
            if let Some(v) = field(cmd, "building") {
                let bid: nd_sim::BuildingId = key_from(v)?;
                let msg = ctx.sim.world.bulldoze_building(bid)?;
                ctx.tools.status = format!("{msg}; its ground goes back to the zoning plan.");
                return Ok(json!({ "message": ctx.tools.status, "treasury": round(ctx.sim.world.economy.treasury) }));
            }
            let id = GestureId(opt_u64(cmd, "gesture_id")?.ok_or("missing gesture_id (or building)")?);
            if ctx.sim.world.gesture(id).is_none() {
                return Err(format!("no gesture {}", id.0));
            }
            ctx.sim.world.remove_gesture(id);
            if opt_bool(cmd, "implement")?.unwrap_or(false) {
                ctx.sim.world.implement();
            }
            ctx.tools.status = "Marked for removal. Ctrl+Enter implements.".into();
            Ok(project_state(ctx))
        }
        "project" => project(cmd, ctx),
        "place" => place(cmd, ctx),
        "plant_preview" => plant_preview(cmd, ctx),
        // [tax] [tolls]
        "taxes" | "tax" => taxes::command(cmd, &mut ctx.sim.world),
        "tolls" | "toll" => taxes::tolls(cmd, &mut ctx.sim.world),
        "incident" => incident(cmd, ctx),
        "incidents" => incidents(cmd, ctx),
        "units" => Ok(units::units_list(&ctx.sim.world, opt_str(cmd, "kind")?, opt_bool(cmd, "out")?.unwrap_or(false), ctx.sim.frac())),
        "society" => society::command(cmd, &mut ctx.sim.world),
        // [social]
        "social" => social::command(cmd, &ctx.sim.world),
        // [events]
        "street_events" | "nights" => nights::command(cmd, &ctx.sim.world),
        "race" => nights::race(cmd, &mut ctx.sim.world),
        "rave" => nights::rave(cmd, &mut ctx.sim.world),
        "roads" | "road_condition" | "road_budget" => nights::roads(cmd, &mut ctx.sim.world),
        "repair_road" => repair_road(cmd, ctx),
        // [interchange] dev: lay the roads again from the plan (as an
        // implement does) with the current rules, nothing in the plan changed.
        "rebuild_roads" => {
            let w = &mut ctx.sim.world;
            let before = w.roads.alive_segments().count();
            w.rebuild_from_plan();
            Ok(json!({ "segments_before": before, "segments": w.roads.alive_segments().count(), "roads_version": w.roads.version }))
        }
        "district" | "districts" => districts::district(cmd, &mut ctx.sim.world, &mut ctx.overlays, &mut ctx.tools),
        "tourism" | "tourists" => districts::tourism(cmd, &mut ctx.sim.world),
        "concerns" => concerns::command(cmd, &ctx.sim.world),
        // [committee] The mayor's committee and its meetings.
        "committee" => committee::command(cmd, &mut ctx.sim, &mut ctx.committee),
        "committee_meeting" | "meeting" => committee::meeting_command(cmd, &mut ctx.sim, &mut ctx.committee),
        // [voices] Diaries on demand; model-written conversation lines.
        "diary" => committee::diary(cmd, &mut ctx.sim, &mut ctx.committee),
        "voices" => committee::voices(cmd, &ctx.sim, &ctx.committee),
        // [land] the land market; ownership highlight data
        "estate" | "land_market" => property::command(cmd, &mut ctx.sim.world),
        "ownership" => property::ownership(cmd, &ctx.sim.world),
        // [supertall] the skyline, height limits, the record book; dev tower starts
        "skyline" | "towers" | "height_limit" => skyline::command(cmd, &mut ctx.sim.world),
        "pressure" | "pressures" => economy::command(cmd, &mut ctx.sim.world),
        "bus" => bus(cmd, ctx),
        "subway" | "metro" => subway::command(cmd, &mut ctx.sim.world),
        // [metro-ux] The metro line tool, click by click.
        "metro_line" => subway::metro_line(cmd, &mut ctx.sim.world, &mut ctx.tools),
        "find" => find(cmd, ctx),
        "inspect" => inspect(cmd, ctx),
        "lane_use" => lane_use(cmd, ctx),
        "intersection" => junction::command(cmd, &mut ctx.sim.world, &mut ctx.tools),
        // [boot] Clamp a private car and have it towed: `car` (on the road:
        // it pulls over first) or `vehicle` (its record, parked or driving).
        "tow_car" | "tow" | "boot" => {
            let w = &mut ctx.sim.world;
            let job = if let Some(v) = field(cmd, "car") {
                w.order_tow_car(key_from(v)?)?
            } else {
                let v = field(cmd, "vehicle").ok_or("car or vehicle is required")?;
                w.order_tow(key_from(v)?)?
            };
            let j = w.enforcement.job(job).copied();
            Ok(json!({ "tow_job": job, "vehicle": j.map(|j| key_str(j.car)), "company": j.and_then(|j| j.company).map(key_str), "out_of_town": j.is_some_and(|j| j.company.is_none()) }))
        }
        "parking_price" => {
            // [parking] The fee at a lot the city runs.
            let id = key_from(field(cmd, "building").ok_or("missing building id")?)?;
            let price = opt_f32(cmd, "price")?.ok_or("missing price")?;
            let set = ctx.sim.world.set_parking_price(id, price)?;
            Ok(json!({ "building": key_str(id), "price": set }))
        }
        "select" => select(cmd, ctx),
        "selected" => Ok(selected_detail(&ctx.sim, *ctx.selection, ctx.tools.route_sel, &ctx.assets, &ctx.tools.favorites)),
        "flags" => flags_cmd(cmd, ctx),
        "flag" => {
            // Flag the current selection from the API, as F8 does in game.
            let note = opt_str(cmd, "note")?.unwrap_or_default().to_string();
            let w = &ctx.sim.world;
            let sel = *ctx.selection;
            let look = ctx.cams.iter().next().map_or([0.0, 0.0], |c| [c.target.x, c.target.z]);
            let snapshot = selected_detail(&ctx.sim, sel, ctx.tools.route_sel, &ctx.assets, &ctx.tools.favorites);
            let brief = selection_json_brief(w, sel);
            let f = ctx.claude.add(w.clock.tick, sel, brief, note, look, snapshot).clone();
            Ok(json!({ "flag": crate::flags::flag_json(&f, None) }))
        }
        "favourite" | "favorite" => {
            let id = key_from(field(cmd, "citizen").ok_or("missing citizen id")?)?;
            if !ctx.sim.world.citizens.contains_key(id) {
                return Err("no such citizen".into());
            }
            let on = opt_bool(cmd, "on")?.unwrap_or(!ctx.tools.favorites.contains(&id));
            ctx.tools.favorites.retain(|f| *f != id);
            if on {
                ctx.tools.favorites.push(id);
            }
            Ok(json!({ "citizen": key_str(id), "favourite": on, "favourites": ctx.tools.favorites.iter().map(|&f| key_str(f)).collect::<Vec<_>>() }))
        }
        "tool" => tool(cmd, ctx),
        "camera" => camera(cmd, ctx),
        // [camera] The inspector's Follow, Zoom to and gift buttons.
        "follow" => follow::follow(cmd, ctx),
        "zoom_to" | "zoom" => follow::zoom_to(cmd, ctx),
        "gift" => follow::gift(cmd, ctx),
        "overlay" => {
            // Map layers are exclusive (`crate::map_layers`): turning one on
            // turns every other off; turning the one showing off leaves none.
            use crate::map_layers::{self as ml, LayerId};
            let o = &mut ctx.overlays;
            let unknown = |k: &str| format!("unknown layer {k:?}; one of none, {}", ml::keys().join(", "));
            let parse = |k: &str| -> Result<Option<LayerId>, String> {
                if k.is_empty() || k.eq_ignore_ascii_case("none") || k.eq_ignore_ascii_case("off") {
                    Ok(None)
                } else {
                    LayerId::parse(k).map(Some).ok_or_else(|| unknown(k))
                }
            };
            let switch = |o: &mut Overlays, l: LayerId, on: bool| {
                if on {
                    ml::set_active(o, Some(l));
                } else if ml::active(o) == Some(l) {
                    ml::set_active(o, None);
                }
            };
            // `{active: "traffic" | "none"}`: the layer to show.
            match cmd.get("active") {
                None => {}
                Some(Value::Null) => ml::set_active(o, None),
                Some(_) => {
                    let k = opt_str(cmd, "active")?.unwrap_or("none");
                    ml::set_active(o, parse(k)?);
                }
            }
            // `{layer, on}` (on omitted toggles).
            if let Some(layer) = opt_str(cmd, "layer")? {
                let on = opt_bool(cmd, "on")?;
                if layer == "projects" {
                    o.projects = on.unwrap_or(!o.projects);
                } else {
                    match parse(layer)? {
                        Some(l) => {
                            let now = ml::active(o) == Some(l);
                            switch(o, l, on.unwrap_or(!now));
                        }
                        None => ml::set_active(o, None),
                    }
                }
            }
            // `{land: "oil"}` shows a land layer, `"none"` (or null) hides
            // whichever land layer shows.
            match cmd.get("land") {
                None => {}
                Some(Value::Null) => {
                    if o.land.is_some() {
                        o.land = None;
                    }
                }
                Some(_) => {
                    let k = opt_str(cmd, "land")?.unwrap_or("none");
                    match parse(k)? {
                        Some(l @ LayerId::Land(_)) => ml::set_active(o, Some(l)),
                        Some(_) => return Err(format!("{k:?} is not a land layer; one of none, {}", LandLayer::ALL.iter().map(|l| l.key()).collect::<Vec<_>>().join(", "))),
                        None => o.land = None,
                    }
                }
            }
            // The older form, a bool per layer name: true shows it (alone).
            for l in ml::MAP_LAYERS {
                if let Some(b) = opt_bool(cmd, l.key)? {
                    switch(o, LayerId::parse(l.key).expect("registry key"), b);
                }
            }
            if let Some(b) = opt_bool(cmd, "projects")? {
                o.projects = b;
            }
            Ok(overlays_json(o))
        }
        "lighting" => {
            let l = &mut ctx.lighting;
            if let Some(h) = opt_f32(cmd, "hour")? {
                l.hour = h.clamp(0.0, 24.0);
                // Asking for an hour means wanting to see it.
                l.follow_clock = false;
            }
            if let Some(b) = opt_bool(cmd, "follow_clock")? {
                l.follow_clock = b;
            }
            Ok(json!({ "follow_clock": l.follow_clock, "hour": l.hour, "heat_vision": l.heat }))
        }
        "heat_vision" | "heat" => {
            let l = &mut ctx.lighting;
            l.heat = opt_bool(cmd, "on")?.unwrap_or(!l.heat);
            Ok(json!({ "heat_vision": l.heat }))
        }
        "graphics" | "quality" => {
            // [look] `{quality: auto|low|medium|high}` pins the tier (for
            // screenshots that must compare) or hands it back to Auto.
            if let Some(q) = opt_str(cmd, "quality")?.or(opt_str(cmd, "tier")?) {
                ctx.lighting.quality = crate::render::QualitySetting::parse(q).ok_or_else(|| format!("unknown quality {q:?}; auto, low, medium or high"))?;
            }
            Ok(graphics_json(ctx))
        }
        "screenshot" => screenshot(cmd, ctx),
        "events" => {
            let since = opt_u64(cmd, "since")?.unwrap_or(0);
            let limit = opt_u64(cmd, "limit")?.unwrap_or(500) as usize;
            let log = &ctx.log;
            let events: Vec<Value> = log.ring.iter().filter(|e| e["seq"].as_u64().unwrap_or(0) > since).take(limit).cloned().collect();
            let dropped = log.ring.front().and_then(|e| e["seq"].as_u64()).is_some_and(|first| since + 1 < first && since > 0);
            Ok(json!({ "seq": log.seq, "events": events, "dropped": dropped, "message": ctx.flags.message, "tool_status": ctx.tools.status }))
        }
        "thoughts" => {
            let limit = opt_u64(cmd, "limit")?.unwrap_or(50) as usize;
            let w = &ctx.sim.world;
            let mut rows: Vec<(u64, Value)> = w.citizens.iter().flat_map(|(id, c)| c.memories.iter().map(move |m| (m.tick, json!({ "citizen": key_str(id), "name": c.name, "tick": m.tick, "text": m.text })))).collect();
            rows.sort_by(|a, b| b.0.cmp(&a.0));
            let memories: Vec<Value> = rows.into_iter().take(limit).map(|(_, v)| v).collect();
            let thoughts: Vec<Value> = w.citizens.iter().filter(|(_, c)| !c.thought.is_empty()).take(limit).map(|(id, c)| json!({ "citizen": key_str(id), "name": c.name, "thought": c.thought })).collect();
            Ok(json!({ "memories": memories, "thoughts": thoughts }))
        }
        "report" => {
            let start = std::time::Instant::now();
            let sections = opt_strings(cmd, "sections")?.unwrap_or_default();
            let mut v = report::report(&ctx.sim.world, &sections)?;
            // The sim's own history rings: asked for by name only, as they
            // are long.
            if sections.iter().any(|s| s == "history") {
                let days = opt_u64(cmd, "history_days")?.unwrap_or(30).clamp(1, 365) as usize;
                v["history"] = report::sim_history(&ctx.sim.world, days);
            }
            v["clock"] = clock(&ctx.sim);
            v["took_ms"] = json!(ms(start));
            Ok(v)
        }
        "industry" => map::industry(cmd, &mut ctx.sim.world),
        "map" => {
            let start = std::time::Instant::now();
            let layer = opt_str(cmd, "layer")?.unwrap_or("summary");
            let mut v = map::map(&ctx.sim.world, opt_f32(cmd, "cell_m")?, layer)?;
            v["took_ms"] = json!(ms(start));
            Ok(v)
        }
        "history" => {
            let keys = opt_strings(cmd, "keys")?;
            let days = opt_u64(cmd, "days")?.map(|d| d as usize);
            ctx.history.to_json(keys.as_deref(), days)
        }
        "list" => {
            let start = std::time::Instant::now();
            let args = list::ListArgs {
                what: req_str(cmd, "what")?,
                sort: opt_str(cmd, "sort")?,
                desc: opt_bool(cmd, "desc")?.unwrap_or(false),
                filter: field(cmd, "filter"),
                fields: opt_strings(cmd, "fields")?,
                offset: opt_u64(cmd, "offset")?.unwrap_or(0) as usize,
                limit: (opt_u64(cmd, "limit")?.unwrap_or(list::DEFAULT_LIMIT as u64) as usize).min(list::MAX_LIMIT),
            };
            let mut v = list::list(&ctx.sim.world, &args)?;
            v["took_ms"] = json!(ms(start));
            Ok(v)
        }
        "export" => export(cmd, ctx),
        "directory" => directory::directory(&ctx.sim.world, &mut ctx.windows, cmd),
        "build_highlight" => build_highlight(cmd, ctx),
        "window" if req_str(cmd, "name").is_ok_and(|n| n.trim().eq_ignore_ascii_case("new_game") || n.trim().eq_ignore_ascii_case("new game")) => {
            let open = opt_bool(cmd, "open")?.unwrap_or(true);
            ctx.windows.new_game = Some(open);
            Ok(json!({ "window": "new game", "open": open }))
        }
        "window" if req_str(cmd, "name").is_ok_and(|n| n.trim().eq_ignore_ascii_case("build")) => {
            // The Build window, optionally at a section.
            let section = match opt_str(cmd, "section")? {
                Some(k) => Some(crate::ui::build::Cat::parse(k).ok_or_else(|| format!("unknown section {k:?}; one of {}", crate::ui::build::Cat::ALL.iter().map(|c| c.key()).collect::<Vec<_>>().join(", ")))?),
                None => None,
            };
            let open = opt_bool(cmd, "open")?.unwrap_or(if section.is_some() { true } else { !ctx.windows.build_open });
            ctx.windows.build = Some((open, section));
            Ok(json!({ "window": "build", "open": open, "section": section.map(|c| c.key()).or(ctx.windows.build_section) }))
        }
        "window" if req_str(cmd, "name").is_ok_and(|n| n.trim().eq_ignore_ascii_case("committee")) => {
            // [committee] The Committee window, optionally at a tab.
            let open = opt_bool(cmd, "open")?.unwrap_or(true);
            let tab = match opt_str(cmd, "tab")? {
                Some(t) => Some(crate::ui::committee::Tab::parse(t).ok_or_else(|| format!("unknown tab {t:?}; meeting, members, alerts, projects, settings"))?),
                None => None,
            };
            ctx.committee.open_request = Some(open);
            ctx.committee.tab_request = tab;
            Ok(json!({ "window": "committee", "open": open, "tab": opt_str(cmd, "tab")? }))
        }
        "window" if req_str(cmd, "name").is_ok_and(|n| matches!(n.trim().to_ascii_lowercase().as_str(), "history" | "game_history" | "game history")) => {
            // [identity] The Game history window.
            let open = opt_bool(cmd, "open")?.unwrap_or(true);
            ctx.saves.history_requested = Some(open);
            Ok(json!({ "window": "game history", "open": open }))
        }
        "window" if req_str(cmd, "name").is_ok_and(|n| matches!(n.trim().to_ascii_lowercase().as_str(), "title" | "title_screen" | "title screen")) => {
            let open = opt_bool(cmd, "open")?.unwrap_or(!ctx.saves.title);
            ctx.saves.title = open;
            if !open {
                // As "Play this map": the city behind runs again.
                ctx.sim.paused = false;
            }
            Ok(json!({ "window": "title", "open": open }))
        }
        "window" if req_str(cmd, "name").is_ok_and(|n| crate::ui::city_data::Page::from_window(n).is_some()) => {
            // [city-data] The report windows are City Data pages now: the old
            // name opens its tab (at its last sub-tab) or its page; People's
            // old `tab` and a `sub` pick a sub-tab. `expanded` is accepted and
            // means open: every page is full size.
            use crate::ui::city_data::{Page, Target};
            let name = req_str(cmd, "name")?;
            let mut target = Page::from_window(name).ok_or("unreachable")?;
            if let Some(sub) = opt_str(cmd, "sub")?.or(opt_str(cmd, "tab")?) {
                let tab = match target {
                    Target::Tab(t) => t,
                    Target::Page(p) => p.tab(),
                };
                let sub = if sub.trim().eq_ignore_ascii_case("overview") && tab == crate::ui::city_data::Tab::People { "population" } else { sub };
                target = Target::Page(Page::parse(None, tab.key(), Some(sub))?);
            }
            let open = if opt_bool(cmd, "expanded")? == Some(true) { Some(true) } else { opt_bool(cmd, "open")? };
            let mut v = ctx.windows.request_data(Some(target), open);
            v["window"] = json!("city data");
            v["note"] = json!("the report windows are tabs of the City Data view now; see city_data");
            Ok(v)
        }
        "window" => {
            let info = window::info_of(req_str(cmd, "name")?)?;
            let (open, list) = ctx.windows.request(info, opt_bool(cmd, "open")?);
            Ok(json!({ "window": info.title().to_ascii_lowercase(), "open": open, "windows_open": list }))
        }
        "city_data" => {
            // [city-data] The full-screen City Data view.
            use crate::ui::city_data::{Page, Target};
            let tab = opt_str(cmd, "tab")?;
            let sub = opt_str(cmd, "sub")?;
            let open = opt_bool(cmd, "open")?;
            if let Some(y) = opt_f32(cmd, "scroll")? {
                ctx.windows.data_scroll = Some(y.max(0.0));
            }
            if tab.is_none() && sub.is_some() {
                return Err("sub needs a tab".into());
            }
            if tab.is_none() && open.is_none() {
                return Ok(ctx.windows.data_json());
            }
            let target = match (tab, sub) {
                (Some(t), Some(s)) => Some(Target::Page(Page::parse(None, t, Some(s))?)),
                (Some(t), None) => Some(Target::Tab(crate::ui::city_data::Tab::parse(t).ok_or_else(|| format!("unknown tab {t:?}; one of city, economy, market, people, services, directory"))?)),
                _ => None,
            };
            // Naming a page opens it unless told otherwise.
            let open = open.or(target.map(|_| true));
            Ok(ctx.windows.request_data(target, open))
        }
        other => Err(format!("unknown command {other:?}; try {{\"cmd\": \"help\"}}")),
    }
}

// ── status ──────────────────────────────────────────────────────────────────

fn clock(sim: &Sim) -> Value {
    let c = &sim.world.clock;
    json!({ "day": c.day() + 1, "time": c.format_hhmm(), "tick": c.tick, "hour": c.hour(), "minute": c.minute() })
}

fn camera_pose(ctx: &Ctx) -> Value {
    match *ctx.mode {
        CameraMode::Walk => {
            let w = &ctx.walk;
            json!({ "mode": "walk", "pos": [round(w.pos.x), round(w.pos.y), round(w.pos.z)], "yaw": round(w.yaw), "pitch": round(w.pitch) })
        }
        CameraMode::Orbit => match ctx.cams.iter().next() {
            Some(cam) => {
                // The eye as `control_camera` will place it from these
                // parameters; the transform itself lags a frame behind a
                // remote camera command.
                let (sy, cy) = cam.yaw.sin_cos();
                let (sp, cp) = cam.pitch.sin_cos();
                let eye = cam.target + Vec3::new(sy * cp, sp, cy * cp) * cam.distance;
                json!({
                    "mode": "orbit", "target": [round(cam.target.x), round(cam.target.z)], "distance": round(cam.distance),
                    "yaw": round(cam.yaw), "pitch": round(cam.pitch), "eye": [round(eye.x), round(eye.y), round(eye.z)],
                })
            }
            None => json!({ "mode": "orbit" }),
        },
    }
}

fn tool_state(t: &ToolState) -> Value {
    json!({
        "name": tool_name(t.tool), "district": { "selected": t.district.selected, "mode": format!("{:?}", t.district.mode).to_lowercase(), "radius": t.district.radius, "erase": t.district.erase, "points": t.district.points.len() }, "road_kind": road_kind_name(t.kind), "lanes_fwd": t.lanes_fwd, "lanes_bwd": t.lanes_bwd, "curved": t.curved,
        "land_use": t.land_use.label(), "zone_mode": t.zone_mode.label(), "brush_radius": t.brush_radius, "unzone": t.unzone, "industry": t.industry, "building": placeable_key(t.build_item()), "building_category": t.build_cat.key(), "rail_kind": rail_name(t.rail_kind),
        "transit_mode": transit_mode_name(t.transit_mode), "route": opt_key(t.route_sel), "show_routes": t.show_routes,
        "pending_points": t.points.len(), "selected_gesture": t.selected_gesture.map(|g| g.0), "status": t.status,
        "elevation": t.elevation, "elevation_label": crate::tools::elevation_label(t.elevation), "point_levels": t.levels(), "tunnel": t.tunnel, "depth": t.depth.label(), "grade_warnings": t.grade_warnings,
        "interchange": t.interchange, "interchange_refusal": t.interchange_refusal.as_ref().map(|(why, at)| json!({ "why": why, "at": [at.x, at.y] })),
        "pillars": junction::pillars_json(&t.pillars), "intersection_editor": t.junction_editor.as_ref().map(|e| v2(e.at)),
    })
}

fn project_state(ctx: &Ctx) -> Value {
    let w = &ctx.sim.world;
    let projects: Vec<Value> = w
        .projects()
        .into_iter()
        .map(|i| {
            json!({
                "id": i.id, "name": i.name, "active": i.active, "steps": i.steps, "redo": i.redo,
                "conflicts": i.conflicts.iter().map(|(g, o)| json!({ "gesture": g.0, "project": o })).collect::<Vec<_>>(),
                "shared_parcels": i.shared_parcels,
            })
        })
        .collect();
    json!({
        "project": w.project.as_ref().map(|p| json!({ "id": p.id, "name": p.name, "steps": p.steps.len(), "redo": p.redo.len() })),
        "projects": projects,
        "warnings": w.plan_warnings,
        // The last implement refused (its roads would take a city
        // structure), and the structures the pending roads run over now.
        "implement_refused": w.implement_refused,
        "blockers": w.project_blockers().iter().map(|b| json!({ "id": key_str(*b), "name": w.buildings.get(*b).map(|x| x.name.clone()) })).collect::<Vec<_>>(),
        "gestures": w.effective_plan().gestures.len(),
        "plan_version": w.plan_version,
    })
}

/// `project {action, id?, name?}`: several named projects, one active.
/// `id` (or `name`) picks the project; without either the active one.
fn project(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let action = req_str(cmd, "action")?.to_ascii_lowercase();
    let name = opt_str(cmd, "name")?;
    let w = &mut ctx.sim.world;
    // The project a command is about: by id, by name, or the active one.
    let target = match opt_u64(cmd, "id")? {
        Some(id) => Some(w.projects().into_iter().find(|i| i.id == id).map(|i| i.id).ok_or_else(|| format!("no project {id}"))?),
        None => match name.filter(|_| action != "new" && action != "rename") {
            Some(n) => Some(w.projects().into_iter().find(|i| i.name.eq_ignore_ascii_case(n.trim())).map(|i| i.id).ok_or_else(|| format!("no project named {n:?}"))?),
            None => w.project.as_ref().map(|p| p.id),
        },
    };
    let need = |t: Option<u64>| t.ok_or_else(|| "no active project; give an id".to_string());
    let did: Value = match action.as_str() {
        "list" => json!(true),
        "start" => {
            w.start_project();
            json!(w.project.as_ref().map(|p| p.id))
        }
        "new" => {
            let id = w.new_project(name);
            ctx.tools.points.clear();
            json!(id)
        }
        "switch" => {
            let id = need(target)?;
            ctx.tools.points.clear();
            ctx.tools.stroke = None;
            json!(w.switch_project(id))
        }
        "park" => json!(w.park_project()),
        "rename" => {
            let id = need(target)?;
            let n = name.ok_or("rename needs a name")?;
            json!(w.rename_project(id, n))
        }
        "delete" => {
            let id = need(target)?;
            ctx.tools.points.clear();
            json!(w.delete_project(id))
        }
        "discard" => {
            let had = target.is_some();
            if let Some(id) = target {
                w.delete_project(id);
            }
            ctx.tools.points.clear();
            json!(had)
        }
        "implement" => {
            let id = need(target)?;
            let name = w.projects().into_iter().find(|i| i.id == id).map_or_else(|| format!("Project {id}"), |i| i.name);
            let roads = if w.project.as_ref().is_some_and(|p| p.id == id) { w.preview().planned_roads.len() } else { 0 };
            let warnings = w.implement_project(id).ok_or_else(|| format!("no project {id}"))?;
            ctx.tools.points.clear();
            match w.implement_refused.clone() {
                // Its roads would take a city structure, or a raised road has
                // no room for its columns: still pending, nothing built, and
                // the reply's `refused` and `blockers` say why.
                Some(why) => {
                    ctx.tools.status = crate::tools::refused_text(&name, roads, &why);
                    json!(false)
                }
                None => {
                    ctx.tools.status = if warnings.is_empty() { "Project implemented".into() } else { format!("Project implemented. {}", warnings.join(" ")) };
                    json!(true)
                }
            }
        }
        "undo" => json!(target.is_some_and(|id| w.undo_project(id))),
        "redo" => json!(target.is_some_and(|id| w.redo_project(id))),
        _ => return Err(format!("unknown project action {action:?}; one of list, new, switch, rename, delete, park, start, implement, undo, redo, discard")),
    };
    let mut v = project_state(ctx);
    v["did"] = did;
    Ok(v)
}

/// `paint {land_use, at+radius | rect | polygon, implement?}`: the zone
/// brush and marquee by remote. `land_use` "none" (or "unzoned") erases.
/// Only parcels that accept the use take it; the rest are listed with why.
fn paint(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let lu_s = req_str(cmd, "land_use")?;
    let land_use = match lu_s.trim().to_ascii_lowercase().as_str() {
        "none" | "unzoned" | "erase" => None,
        other => Some(land_use_of(other)?),
    };
    let w = &mut ctx.sim.world;
    let ids: Vec<usize> = if let Some(at) = opt_vec2(cmd, "at")? {
        let r = opt_f32(cmd, "radius")?.unwrap_or(ctx.tools.brush_radius).clamp(BRUSH_MIN, BRUSH_MAX * 4.0);
        w.parcels.in_circle(at, r)
    } else if let Some(rect) = field(cmd, "rect") {
        let pts = rect.as_array().filter(|a| a.len() == 2).ok_or("rect must be [[x0, y0], [x1, y1]]")?;
        let (a, b) = (vec2_of(&pts[0])?, vec2_of(&pts[1])?);
        w.parcels.in_polygon(&marquee_rect(a, b, 0.0))
    } else if field(cmd, "polygon").is_some() {
        w.parcels.in_polygon(&req_points(cmd, "polygon")?)
    } else {
        return Err("give at (+ radius), rect or polygon".into());
    };
    let refused: Vec<Value> = match land_use {
        Some(lu) => ids.iter().filter_map(|&i| w.parcel_refusal(i, Some(lu)).map(|why| json!({ "parcel": i, "center": v2(w.parcels.parcels[i].center), "why": why }))).collect(),
        None => Vec::new(),
    };
    // [island-lots] Nothing under the brush: why (water, too far from a
    // road, past the buildable land), as the hover says at the cursor.
    let none_why = if ids.is_empty() { opt_vec2(cmd, "at")?.and_then(|at| w.no_parcel_why(at)) } else { None };
    let industry = match opt_str(cmd, "industry")? {
        Some(k) => Some(nd_sim::world::INDUSTRY_CHOICES.iter().map(|(key, _)| *key).find(|key| key.eq_ignore_ascii_case(k.trim())).ok_or_else(|| format!("unknown industry {k:?}; one of {}", nd_sim::world::INDUSTRY_CHOICES.map(|c| c.0).join(", ")))?),
        None => None,
    };
    let (gesture, painted) = w.paint_parcels_with(&ids, land_use, industry, None);
    let implemented = opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        w.implement();
    }
    let what = land_use.map_or("unzoned".to_string(), |l| format!("zoned {}", l.label()));
    ctx.tools.status = format!("{painted} parcels {what} by remote{}", if implemented { " and implemented" } else { " (pending)" });
    let mut v = project_state(ctx);
    v["gesture_id"] = json!(gesture.map(|g| g.0));
    v["touched"] = json!(ids.len());
    v["painted"] = json!(painted);
    v["refused"] = json!(refused);
    if let Some(why) = none_why {
        v["no_parcel"] = json!(why);
    }
    v["implemented"] = json!(implemented && ctx.sim.world.implement_refused.is_none());
    Ok(v)
}

fn status(ctx: &Ctx) -> Value {
    let sim = &ctx.sim;
    let w = &sim.world;
    let e = &w.economy;
    let p = &e.power;
    let t = &e.transit;
    let r = &w.rail.stats;
    let sites = w.buildings.values().filter(|b| b.construction.is_some()).count();
    let fps = ctx.diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let ledger: serde_json::Map<String, Value> = e.ledger.yesterday_sorted().into_iter().map(|(f, x)| (f.label().to_string(), json!(round(x)))).collect();
    let power_rows = report::power_json(w, false);
    let mut v = json!({
        "clock": clock(sim), "paused": sim.paused, "speed_index": sim.speed_index, "speed": sim.speed(), "speed_label": SPEEDS[sim.speed_index].0,
        "effective_speed": round(sim.effective_speed), "ticks_last_frame": sim.ticks_last_frame,
        "population": w.citizens.len(), "buildings": w.buildings.len() - sites, "under_construction": sites,
        "lots": w.lots.len(), "lots_vacant": w.lots.values().filter(|l| l.building.is_none() && l.usable()).count(), "lots_unusable": w.lots.values().filter(|l| l.landlocked).count(),
        "homes": { "used": e.homes_total - e.homes_free, "total": e.homes_total }, "jobs": { "used": e.jobs_total - e.jobs_free, "total": e.jobs_total },
        "waiting_immigrants": e.waiting_immigrants, "unhoused": e.unhoused, "unemployed": e.unemployed,
        "treasury": round(e.treasury), "tax_today": round(e.tax_today), "ledger_yesterday": ledger,
        "power": {
            "supply_mw": round(p.supply_mw), "imported_mw": round(p.imported_mw), "exported_mw": round(p.exported_mw), "demand_mw": round(p.demand_mw), "served": round(p.served),
            "plants": p.plants, "unpowered_buildings": p.unpowered_buildings, "brownout_hours_today": p.brownout_hours_today, "upkeep_today": round(p.upkeep_today),
            "import_cost_today": round(p.import_cost_today), "export_income_today": round(p.export_income_today),
            "import_price": round(p.import_price), "export_price": round(p.export_price),
            "batteries": { "count": p.batteries, "charge_mwh": round(p.battery_charge_mwh), "capacity_mwh": round(p.battery_capacity_mwh), "charging_mw": round(p.battery_in_mw), "discharging_mw": round(p.battery_out_mw) },
            "rows": { "plants": power_rows["plants"], "batteries": power_rows["batteries"] },
        },
        "weather": land::weather_json(w),
        "land": { "avg_land_value": round(e.avg_land_value), "gini": round(e.gini), "displaced_total": e.displaced_total, "emigrated_total": e.emigrated_total, "priced_out_total": e.priced_out_total, "tier_counts": e.tier_counts, "class_counts": e.class_counts, "avg_rent": e.avg_rent.map(round) },
        "market": { "businesses": e.market.businesses, "deals_today": e.market.deals_today, "imports_today": round(e.market.imports_today), "exports_today": round(e.market.exports_today), "meals_sold_today": e.market.meals_sold_today, "hungry": e.market.hungry, "closed_total": e.market.closed_total },
        "demand": nd_sim::LandUse::ALL.iter().map(|&lu| (lu.label().to_string(), json!(round(e.demand_for(lu))))).collect::<serde_json::Map<_, _>>(),
        "traffic": { "cars": w.traffic.cars.len(), "lanes": w.traffic.net.lanes.len(), "signals": w.traffic.net.intersections.iter().filter(|i| i.signalized).count(), "parking_used": e.parking_used, "parking_capacity": e.parking_capacity, "walk_trips_today": e.walk_trips_today, "pedestrians": { "waiting_at_crossings": w.peds.waiting_now, "jaywalkers_today": w.peds.jaywalkers_today, "near_misses_today": w.peds.near_misses_today, "ped_collisions_today": w.peds.collisions_today } },
        "bots": { "active": w.bots.bots.len(), "delivered_today": w.bots.delivered_today, "vans_on_road": nd_sim::Bots::vans(w), "van_orders_today": w.bots.van_orders_today },
        "transit": { "routes": w.transit.routes.len(), "buses": t.buses, "depots": t.depots, "depot_capacity": t.depot_capacity, "waiting": t.waiting, "riding": t.riding, "riders_today": t.riders_today, "fares_today": round(t.fares_today), "upkeep_today": round(t.upkeep_today) },
        "rail": { "trains": r.trains, "stations": r.stations, "yards": r.yards, "waiting": r.waiting, "riding": r.riding, "riders_today": r.riders_today, "fares_today": round(r.fares_today), "next_passenger_in": r.next_passenger_in, "freight_in_today": round(r.freight_in_today), "freight_waiting": round(r.freight_waiting), "upkeep_today": round(r.upkeep_today) },
        "render": { "fps": round(fps as f32), "ticks_per_frame": sim.ticks_last_frame, "triangles": ctx.stats.street_triangles + ctx.stats.building_triangles, "building_meshes": ctx.stats.building_entities, "graphics": graphics_json(ctx) },
        "camera": camera_pose(ctx), "tool": tool_state(&ctx.tools), "selection": selection(w, *ctx.selection),
        "project": project_state(ctx)["project"], "overlays": overlays_json(&ctx.overlays),
        "lighting": { "follow_clock": ctx.lighting.follow_clock, "hour": round(ctx.lighting.hour), "heat_vision": ctx.lighting.heat },
        "message": ctx.flags.message, "tool_status": ctx.tools.status, "event_seq": ctx.log.seq,
        "favourites": ctx.tools.favorites.iter().map(|&f| key_str(f)).collect::<Vec<_>>(),
    });
    // [camera] What the camera is locked on, if anything.
    v["following"] = json!(ctx.follow.target.map(|s| selection(w, s)));
    // [subway] Added apart: the literal above is at serde_json's macro depth.
    let ss = &w.subway.stats;
    v["subway"] = json!({ "stations": ss.stations, "lines": ss.lines, "trains": ss.trains, "waiting": ss.waiting, "riding": ss.riding, "riders_today": ss.riders_today, "riders_yesterday": ss.riders_yesterday, "fares_today": round(ss.fares_today) });
    // Added after the literal: one `json!` this size hits the macro recursion limit.
    v["windows_open"] = json!(ctx.windows.open_names());
    v["city_data"] = ctx.windows.data_status();
    v["build_window"] = json!({ "open": ctx.windows.build_open, "section": ctx.windows.build_section });
    v["title_screen"] = json!(ctx.saves.title);
    // [climate] The map's climate and what its weather is doing to the view.
    v["climate"] = json!({ "climate": w.landscape.params.climate.key(), "label": w.landscape.params.climate.label(), "precipitation": crate::render::climate::precip_now(w).map(|(k, i)| json!({ "kind": k, "intensity": round(i) })) });
    // [identity]
    v["identity"] = identity::identity_json(w);
    // [looks] The figure cache: meshes alive, people outdoors and their distinct looks.
    let st = &ctx.stats;
    v["render"]["people"] = json!({ "meshes": st.people_meshes, "mesh_cap": crate::render::PEOPLE_MESH_CAP, "visible": st.people_visible, "distinct_looks_visible": st.people_looks_visible, "built_total": st.people_built, "borrowed": st.people_borrowed, "materials": 1 });
    v["alerts_top"] = report::alerts_json(&report::alerts(w), 5);
    // [districts] [tourism]
    let ts = &w.tourism;
    v["districts"] = json!(w.districts.list.len());
    v["tourism"] = json!({ "tourists": ts.count(), "attractiveness": round(ts.attractiveness), "arrivals_today": ts.stats.arrivals_today, "turned_away_today": ts.stats.turned_away_today, "spend_today": round(ts.stats.spend_today()), "tourist_tax_yesterday": round(ts.stats.tax_yesterday), "hotels": w.hotels().len() });
    v["review_hint"] = json!("for a whole-city review call report (then map, history, list)");
    v
}

// ── saves ───────────────────────────────────────────────────────────────────

fn save(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    if let Some(name) = opt_str(cmd, "name")? {
        // A named game: saves/<file-safe name>.ron, its sidecar and picture.
        let Ctx { sim, saves, flags, .. } = ctx;
        let path = crate::ui::save_as(sim, saves, flags, name)?;
        let p = slash(&path);
        return Ok(json!({ "path": absolute(&p), "name": name.trim(), "thumbnail": absolute(&p.replace(".ron", ".png")), "meta": absolute(&p.replace(".ron", ".meta.json")), "note": "the thumbnail is written about half a second later (rendered offscreen)", "message": flags.message, "identity": identity::identity_json(&sim.world) }));
    }
    match opt_str(cmd, "path")? {
        Some(raw) => {
            let path = allowed_path(raw, "ron", true)?;
            let shown = path.to_string_lossy().replace('\\', "/");
            crate::ui::saves::ensure_game_id(&mut ctx.sim.world);
            ctx.sim.save(&path)?;
            ctx.saves.mark_dirty();
            ctx.flags.message = format!("saved {shown}");
            Ok(json!({ "path": shown }))
        }
        None => {
            let Ctx { sim, saves, flags, .. } = ctx;
            save_now(sim, saves, flags);
            if flags.message.starts_with("save failed") {
                return Err(flags.message.clone());
            }
            Ok(json!({ "path": absolute(crate::ui::LATEST), "thumbnail": absolute("saves/city.png"), "message": flags.message }))
        }
    }
}

/// A path with forward slashes.
fn slash(p: &std::path::Path) -> String {
    p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

/// Every save, newest first, with what its sidecar knows and its picture.
fn list_saves() -> Value {
    let slots = crate::ui::saves::scan();
    let rows: Vec<Value> = slots
        .iter()
        .map(|s| {
            let modified = s.modified.and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            let bytes = std::fs::metadata(&s.path).map_or(0, |m| m.len());
            json!({
                "path": slash(&s.path),
                "name": s.label,
                "kind": s.kind.key(),
                "modified_unix": modified,
                "bytes": bytes,
                "day": s.meta.as_ref().map(|m| m.day),
                "population": s.meta.as_ref().map(|m| m.population),
                "treasury": s.meta.as_ref().map(|m| round(m.treasury)),
                "buildings": s.meta.as_ref().map(|m| m.buildings),
                "landscape": s.meta.as_ref().map(|m| m.landscape.clone()),
                "thumbnail": s.picture.as_ref().filter(|p| p.exists()).map(|p| absolute(&slash(p))),
                "city": s.city(),
                "game_id": s.meta.as_ref().map(|m| m.game_id.clone()).filter(|g| !g.is_empty()),
            })
        })
        .collect();
    // [identity] The same saves grouped by city name, as the title screen
    // and the Saves window show them.
    let groups: Vec<Value> = crate::ui::saves::group_by_city(&slots).into_iter().map(|(city, members)| json!({ "city": city, "saves": members.iter().map(|&i| slash(&slots[i].path)).collect::<Vec<_>>() })).collect();
    json!({ "saves": rows, "by_city": groups, "latest": crate::ui::LATEST })
}

/// A path a file command may use (see `guard::confine`): inside saves/,
/// screenshots/ or exports/ under the working directory, or the temp
/// folder, ending in `.ext`. `write` makes its folder.
fn allowed_path(raw: &str, ext: &str, write: bool) -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let path = guard::confine(raw, ext, &cwd, &std::env::temp_dir())?;
    if write {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
    }
    Ok(path)
}

fn absolute(path: &str) -> String {
    let p = PathBuf::from(path);
    let abs = if p.is_absolute() { p } else { std::env::current_dir().map(|d| d.join(p)).unwrap_or_default() };
    abs.to_string_lossy().replace('\\', "/")
}

// ── planning ────────────────────────────────────────────────────────────────

fn plan(ctx: &Ctx) -> Value {
    let w = &ctx.sim.world;
    let gestures: Vec<Value> = w.effective_plan().gestures.iter().map(|(id, g)| gesture(*id, g)).collect();
    // Which of them are pending rather than built: anything the project touches.
    let pending: Vec<u64> = w.project.as_ref().map(|p| p.steps.iter().flat_map(|s| s.keys().map(|k| k.0)).collect()).unwrap_or_default();
    let mut v = project_state(ctx);
    v["gestures"] = json!(gestures);
    v["pending_gesture_ids"] = json!(pending);
    v["master_gestures"] = json!(w.master.gestures.len());
    // [tools] What implementing the active project would refuse on, checked
    // now as implementing checks it (the trial rebuild included).
    let check = w.active_project_check();
    // Exactly what Implement would answer now (None: it implements).
    v["will_refuse"] = json!(check.refusal);
    // Every structure it would refuse on, with the road that takes it.
    v["blockers"] = json!(check
        .blocks
        .iter()
        .map(|k| json!({
            "id": key_str(k.building), "name": k.name, "at": [k.at.x, k.at.y],
            "road_gesture": k.road.map(|g| g.0), "road": k.road_label,
            "stretch": [[k.a.x, k.a.y], [k.b.x, k.b.y]],
            "runs_over": k.runs_over, "reason": k.reason(),
        }))
        .collect::<Vec<_>>());
    v["pillar_gaps"] = json!(check.pillar_gaps.iter().map(junction::gap_json).collect::<Vec<_>>());
    v
}

fn add_road(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let (mut pts, given) = view::req_points_levels(cmd, "points")?;
    // `elevation`: one level for every point without its own, or a list.
    let all = opt_f32(cmd, "elevation").ok().flatten();
    let list: Option<Vec<f32>> = field(cmd, "elevation").and_then(Value::as_array).map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect());
    let tunnel = opt_bool(cmd, "tunnel")?.unwrap_or(false);
    let base = all.unwrap_or(if tunnel { nd_sim::roads::TUNNEL_DEPTH } else { 0.0 });
    let mut levels: Vec<f32> = given.iter().enumerate().map(|(i, l)| l.or_else(|| list.as_ref().and_then(|v| v.get(i).copied())).unwrap_or(base)).collect();
    if pts.len() < 2 {
        return Err("a road needs at least two points".into());
    }
    let kind = opt_str(cmd, "kind")?.map(road_kind_of).transpose()?.unwrap_or(RoadKind::Street);
    // [park-paths] A path may start or end on a grounds gate by its index
    // in `grounds_junctions`.
    for (key, first) in [("from_junction", true), ("to_junction", false)] {
        if let Some(i) = opt_u64(cmd, key)? {
            if kind != RoadKind::Path {
                return Err(format!("{key}: only a path joins a park's walkways"));
            }
            let j = ctx.sim.world.grounds_junctions().get(i as usize).ok_or_else(|| format!("{key}: no junction {i} (see grounds_junctions)"))?;
            let at = if first { 0 } else { pts.len() - 1 };
            pts[at] = j.pos;
            levels[at] = 0.0;
        }
    }
    // Range-checked as u64 before narrowing: `as u8` wrapped 257 to 1, and
    // a u8 sum of 200 + 100 overflowed (a panic in a dev build).
    let (lf, lb) = if kind == RoadKind::Street { (opt_u64(cmd, "lanes_fwd")?.unwrap_or(1), opt_u64(cmd, "lanes_bwd")?.unwrap_or(1)) } else { (0, 0) };
    if lf > 4 || lb > 4 {
        return Err("lanes are 0..4 per direction".into());
    }
    let (lf, lb) = (lf as u8, lb as u8);
    if kind == RoadKind::Street && lf + lb == 0 {
        return Err("a street needs at least one lane".into());
    }
    // Snap the ends to the network like the Road tool does, so the road
    // joins existing roads instead of stopping a metre short of them.
    // Points snap onto a road at about their level; an end given no level of
    // its own takes the level of the road it snaps onto (a ramp or portal).
    if opt_bool(cmd, "snap")?.unwrap_or(true) {
        let last = pts.len() - 1;
        for (i, p) in pts.iter_mut().enumerate() {
            let end = (i == 0 || i == last) && given[i].is_none();
            let (q, snapped, l) = ToolState::road_snap_level(&ctx.sim, *p, kind, levels[i], end);
            *p = q;
            if snapped {
                levels[i] = l;
            }
        }
    }
    let curved = opt_bool(cmd, "curved")?.unwrap_or(false) && pts.len() >= 3;
    let warnings = ctx.sim.world.road_grade_warnings(&pts, &levels, kind, lf, lb);
    // As the Road tool: a crossing with the railway off grade is refused.
    let (bp, bl) = if curved { (nd_sim::plan::sample_curve(&pts), nd_sim::plan::sample_curve_levels(&pts, &levels)) } else { (pts.clone(), levels.clone()) };
    if let Some(why) = ctx.sim.world.road_refusal(&bp, &bl, kind, lf, lb) {
        return Err(why);
    }
    let elevation = if levels.iter().any(|l| *l != 0.0) { levels } else { Vec::new() };
    let id = ctx.sim.world.add_gesture(Gesture::Road { points: pts, lanes_fwd: lf, lanes_bwd: lb, curved, kind, elevation });
    let implemented = opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        ctx.sim.world.implement();
    }
    ctx.tools.status = match &ctx.sim.world.implement_refused {
        Some(why) if implemented => why.clone(),
        _ => format!("{} added by remote{}", match kind { RoadKind::Path => "path", RoadKind::Rail => "track", RoadKind::Street => "road" }, if implemented { " and implemented" } else { " (pending)" }),
    };
    let mut v = project_state(ctx);
    v["gesture_id"] = json!(id.0);
    v["implemented"] = json!(implemented && ctx.sim.world.implement_refused.is_none());
    if kind == RoadKind::Path {
        // [park-paths] Ends on a grounds gate (joined once implemented).
        let ends = match ctx.sim.world.gesture(id) {
            Some(Gesture::Road { points, .. }) if points.len() >= 2 => vec![points[0], points[points.len() - 1]],
            _ => Vec::new(),
        };
        let w = &ctx.sim.world;
        v["joined"] = json!(ends
            .iter()
            .filter_map(|p| w.grounds_junctions().iter().position(|j| j.pos.distance(*p) < 1.0))
            .map(|i| grounds::junction_json(w, i))
            .collect::<Vec<_>>());
    }
    v["grade_warnings"] = json!(warnings);
    Ok(v)
}

/// `add_roundabout {center, radius | diameter, lanes, implement?}`: stamp a
/// roundabout into the project as the Road tool does (the centre snaps onto
/// a junction or street). Reply: what it cuts and joins and the buildings
/// it would take, besides the project state.
fn add_roundabout(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let center = view::opt_vec2(cmd, "center")?.map_or_else(|| view::req_point(cmd), Ok)?;
    let radius = match (opt_f32(cmd, "radius")?, opt_f32(cmd, "diameter")?) {
        (Some(r), _) => r,
        (None, Some(d)) => d * 0.5,
        (None, None) => ctx.tools.ring_radius,
    };
    let lanes = opt_u64(cmd, "lanes")?.map_or(ctx.tools.ring_lanes, |n| u8::try_from(n).unwrap_or(u8::MAX));
    if !(1..=nd_sim::roads::RING_MAX_LANES).contains(&lanes) {
        return Err("a roundabout has 1, 2 or 3 ring lanes".into());
    }
    let id = ctx.sim.world.add_roundabout(center, radius, lanes)?;
    let ring = ctx.sim.world.gesture(id).and_then(|g| g.ring()).ok_or("roundabout not in the plan")?;
    let fx = nd_sim::plan::ring_effect(ctx.sim.world.effective_plan(), ring);
    let takes: Vec<Value> = ctx.sim.world.roundabout_takes(ring).into_iter().filter_map(|l| ctx.sim.world.lots.get(l).and_then(|x| x.building)).map(|b| json!(view::key_str(b))).collect();
    let implemented = opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        ctx.sim.world.implement();
    }
    ctx.tools.status = format!("Roundabout {:.0} m across added by remote{}", ring.radius * 2.0, if implemented { " and implemented" } else { " (pending)" });
    let mut v = project_state(ctx);
    v["gesture_id"] = json!(id.0);
    v["implemented"] = json!(implemented && ctx.sim.world.implement_refused.is_none());
    v["roundabout"] = json!({
        "center": [ring.center.x, ring.center.y], "radius": ring.radius, "diameter": ring.radius * 2.0, "lanes": ring.lanes,
        "joins": fx.joins.iter().map(|p| json!([p.x, p.y])).collect::<Vec<_>>(),
        "cuts_roads": fx.cut_roads.iter().map(|g| g.0).collect::<Vec<_>>(),
        "takes_buildings": takes,
    });
    Ok(v)
}

/// [interchange] `add_interchange {from, to, lanes_fwd?, lanes_bwd?, check?,
/// implement?}`: a diamond interchange for a cross street drawn from `from`
/// to `to` across the highway, as the Road tool's Interchange preset lays
/// it. `check` only plans it (nothing is added). Refused with `refused`
/// {why, at}: the spot to look at.
fn add_interchange(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let from = view::opt_vec2(cmd, "from")?.ok_or("from [x,y] is required")?;
    let to = view::opt_vec2(cmd, "to")?.ok_or("to [x,y] is required")?;
    let f = opt_u64(cmd, "lanes_fwd")?.map_or(2, |n| n.min(4) as u8);
    let b = opt_u64(cmd, "lanes_bwd")?.map_or(2, |n| n.min(4) as u8);
    let check = opt_bool(cmd, "check")?.unwrap_or(false);
    let w = &mut ctx.sim.world;
    let planned = if check { w.plan_diamond(from, to, (f, b)).map(|p| (p, Vec::new())) } else { w.add_diamond(from, to, (f, b)) };
    let (plan, ids) = match planned {
        Ok(x) => x,
        Err(r) => {
            let mut v = project_state(ctx);
            v["refused"] = json!({ "why": r.why, "at": [r.at.x, r.at.y] });
            v["implemented"] = json!(false);
            return Ok(v);
        }
    };
    let implemented = !check && opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        ctx.sim.world.implement();
    }
    if !check {
        ctx.tools.status = format!("Diamond interchange added by remote{}", if implemented { " and implemented" } else { " (pending)" });
    }
    let mut v = project_state(ctx);
    v["gesture_ids"] = json!(ids.iter().map(|g| g.0).collect::<Vec<_>>());
    v["implemented"] = json!(implemented && ctx.sim.world.implement_refused.is_none());
    let pts = |g: &Gesture| match g {
        Gesture::Road { points, lanes_fwd, lanes_bwd, .. } => json!({ "points": points.iter().map(|p| [round(p.x), round(p.y)]).collect::<Vec<_>>(), "lanes_fwd": lanes_fwd, "lanes_bwd": lanes_bwd }),
        _ => json!(null),
    };
    v["interchange"] = json!({
        "kept_street": plan.kept_street,
        "terminals": plan.terminals.iter().map(|p| [round(p.x), round(p.y)]).collect::<Vec<_>>(),
        "crossings": plan.crossings.iter().map(|p| [round(p.x), round(p.y)]).collect::<Vec<_>>(),
        "roads": plan.gestures.iter().map(pts).collect::<Vec<_>>(),
    });
    Ok(v)
}

fn add_zone(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let polygon = req_points(cmd, "polygon")?;
    if polygon.len() < 3 {
        return Err("a zone needs at least three corners".into());
    }
    let land_use = land_use_of(req_str(cmd, "land_use")?)?;
    let id = ctx.sim.world.add_gesture(Gesture::Zone { polygon, land_use });
    let implemented = opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        ctx.sim.world.implement();
    }
    ctx.tools.status = format!("{} zone added by remote{}", land_use.label(), if implemented { " and implemented" } else { " (pending)" });
    let mut v = project_state(ctx);
    v["gesture_id"] = json!(id.0);
    v["implemented"] = json!(implemented && ctx.sim.world.implement_refused.is_none());
    Ok(v)
}

// ── placing ─────────────────────────────────────────────────────────────────

/// The lot under `p`, or failing that the nearest vacant one within reach:
/// a script aims at a block, not at a parcel's exact polygon.
fn lot_near(w: &nd_sim::World, p: SVec2) -> Option<LotId> {
    if let Some((id, _)) = w.lots.iter().find(|(_, l)| point_in_polygon(p, &l.polygon)) {
        return Some(id);
    }
    w.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable()).map(|(id, l)| (id, l.center.distance(p))).filter(|(_, d)| *d < 80.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
}

fn place(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let kind = req_str(cmd, "kind")?.trim().to_ascii_lowercase();
    let p = req_point(cmd)?;
    let w = &mut ctx.sim.world;
    let (id, note) = match kind.as_str() {
        "station" | "freight_yard" | "yard" | "central_station" | "central" | "main_station" => {
            let fac = match kind.as_str() {
                "station" => RailFacility::Station,
                "freight_yard" | "yard" => RailFacility::FreightYard,
                _ => RailFacility::Central,
            };
            let id = w.place_rail_facility(fac, p)?;
            let link = w.buildings.get(id).and_then(|b| b.rail.as_ref()).map(|l| (l.rail_connected, l.road_connected));
            let note = match link {
                Some((true, true)) => "placed and connected",
                Some((true, false)) => "placed on the track; trains call, nobody gets on or off until a road reaches the marked end of its street stub (road_access.stub_ends)",
                Some((false, true)) => "placed by the street; lay track onto its rail stub",
                Some((false, false)) => "placed; needs track to its rail stub and a road to its street stub",
                None => "placed",
            };
            (id, note.to_string())
        }
        "parking" => {
            let lot = lot_near(w, p).ok_or("no lot there")?;
            (w.build_parking(lot).ok_or("cannot build parking there: the lot is taken, landlocked or too small")?, "parking built".into())
        }
        // [tolls] Across the road at the point: not a building.
        other if nd_sim::tolls::TollSize::from_key(other).is_some() => {
            let size = nd_sim::tolls::TollSize::from_key(other).unwrap();
            let id = w.place_toll(p, size)?;
            let row = w.tolls.get(id).map(|b| taxes::booth_row(w, b)).unwrap_or(Value::Null);
            let note = format!("{} built for {:.0}; {:.0}/day; toll {:.1} (lorries double)", size.label(), size.cost(), size.upkeep_per_day(), size.default_price());
            ctx.tools.status = note.clone();
            return Ok(json!({ "toll": id, "message": note, "row": row, "treasury": round(w.economy.treasury) }));
        }
        other if land::dock_of(other).is_some() => {
            let dock = land::dock_of(other).unwrap();
            let id = w.place_port(dock, p)?;
            let port = w.ports.port_of(id).cloned();
            let note = match port {
                Some(p) if p.works() => format!("{} built with its pier; ships call from the next sailing", dock.label()),
                Some(p) if !p.road_connected => format!("{} built with its pier and an access road; not connected to the road network: draw a road to the marked end of its stub (road_access.stub_ends)", dock.label()),
                _ => format!("{} built, but no ship can reach it from off the map", dock.label()),
            };
            (id, note)
        }
        other => {
            let tier = opt_str(cmd, "tier")?.map(|t| nd_sim::services::parse_tier(t).ok_or_else(|| format!("unknown tier {t:?}; low, mid or high"))).transpose()?;
            let what = nd_sim::Placeable::parse(other, tier).ok_or_else(|| format!("unknown kind {other:?}; one of station, freight_yard, depot, parking, coal, solar, wind, nuclear, hydro, tidal, geothermal, battery_small, battery_medium, battery_large, police, fire, hospital (clinic), school, dispatch, monument, stadium, vertiport (services take tier low|mid|high or a :tier suffix)"))?;
            // [free-placement] Any lot or unzoned ground with road access at
            // the point (a lot is made for it there); off both, the nearest
            // vacant lot within 80 m. `check: true` only says what would happen.
            let site = w.placement_site(what, p);
            if opt_bool(cmd, "check")?.unwrap_or(false) {
                return Ok(match (site, w.placement_problem_at(what, p)) {
                    (Ok(site), Ok(_)) => json!({ "ok": true, "note": site.note(), "lot": site.lot.map(key_str), "parcels": site.parcels.len(), "leaves_zone": site.leaves_zone.map(|l| l.label()), "outline": points(&site.outline) }),
                    (_, Err(why)) => json!({ "ok": false, "reason": why }),
                    (Err(why), _) => json!({ "ok": false, "reason": why }),
                });
            }
            let before = w.economy.treasury;
            let (id, here) = match site {
                Ok(site) => (w.place_building(what, p)?, site.note()),
                Err(_) => {
                    let lot = lot_near(w, p).ok_or("no lot or usable parcel there: aim at ground along a road")?;
                    (w.place_on_lot(what, lot)?, "nearest lot".into())
                }
            };
            // [plant-groups] What was paid, and a plant's MW on its lot and its park.
            let paid = before - w.economy.treasury;
            let note = match w.buildings.get(id).filter(|b| b.plant.is_some()) {
                Some(b) => {
                    let mut s = format!("{} built: {:.1} MW for {:.0}; {:.0}/day ({here})", what.label(), w.plant_capacity_mw(b), paid, w.plant_upkeep(b));
                    if let Some(g) = w.plant_group_of(id) {
                        s += &format!("; part of {} ({} lots)", g.name, g.members.len());
                    }
                    s
                }
                None => format!("{} built for {:.0}; {:.0}/day ({here})", what.label(), paid, what.upkeep()),
            };
            (id, note)
        }
    };
    *ctx.selection = Selection::Building(id);
    ctx.tools.status = note.clone();
    let b = ctx.sim.world.buildings.get(id).map(|b| building_row(&ctx.sim.world, id, b)).unwrap_or(Value::Null);
    let mut out = json!({ "building": key_str(id), "message": note, "row": b, "treasury": round(ctx.sim.world.economy.treasury) });
    // [dock-access] A dock or station: connected, and the stub's free end.
    if let Some(connected) = ctx.sim.world.road_access(id) {
        out["road_access"] = view::access_json(&ctx.sim.world, id, connected);
    }
    Ok(out)
}

/// [plant-groups] What a plant `kind` would be on the lot at `x`,`y`
/// before placing it: its MW, cost and upkeep there, the Sunfield or Wind
/// Park it would join, and why it cannot go (if not). Read-only.
fn plant_preview(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let kind = req_str(cmd, "kind")?.trim().to_ascii_lowercase();
    let p = req_point(cmd)?;
    let w = &ctx.sim.world;
    let Some(nd_sim::Placeable::Plant(k)) = nd_sim::Placeable::parse(&kind, None) else { return Err(format!("{kind:?} is not a power plant")) };
    // [plant-scale] The site a click there takes: a lot, or unzoned ground
    // (`placement_site`), so the figures are what the click charges.
    let site = w.placement_site(nd_sim::Placeable::Plant(k), p)?;
    let v = w.plant_preview_site(k, &site).ok_or("no road access to front")?;
    Ok(json!({
        "lot": site.lot.map(key_str), "unzoned_parcels": site.parcels.len(), "kind": kind, "area_m2": round(v.area), "size_scale": round(v.scale), "capacity_mw": round(v.capacity_mw), "site": round(v.site), "now_mw": round(v.now_mw),
        "cost": round(v.cost), "upkeep_per_day": round(v.upkeep_per_day), "min_area": k.min_area(), "max_area": k.max_area(), "small_lot_joins": v.small_lot_joins,
        "joins": v.join.as_ref().and_then(|j| j.joins.clone()), "joined_lots": v.join.as_ref().map(|j| j.members.len() + 1), "joined_area_m2": v.join.as_ref().map(|j| round(j.area)), "joined_capacity_mw": v.join.as_ref().map(|j| round(j.capacity_mw)), "adds_mw": v.join.as_ref().map(|j| round(j.capacity_mw - j.before_mw)),
        "outline": view::points(&site.outline),
        "ok": v.problem.is_none(), "problem": v.problem,
    }))
}

/// Start an emergency by hand, for testing and demos: `kind` fire / crime
/// / medical at a `building` (medical picks someone inside it, or `citizen`).
fn incident(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let kind = req_str(cmd, "kind")?.trim().to_ascii_lowercase();
    let w = &mut ctx.sim.world;
    let id = match kind.as_str() {
        "fire" | "crime" => {
            let b = key_from(field(cmd, "building").ok_or("building is required")?)?;
            if !w.buildings.contains_key(b) {
                return Err("no such building".into());
            }
            let id = if kind == "fire" { w.start_fire(b) } else { w.start_crime(b) }.ok_or("cannot start that here (already burning, a site, a park or a station)")?;
            // [fire-scene] Dev: start it already this big (0..1), for a fire
            // that takes the engine a while to knock down.
            if let Some(k) = opt_f32(cmd, "intensity")?.filter(|_| kind == "fire") {
                if let Some(i) = w.services.incident_mut(id) {
                    i.intensity = k.clamp(0.01, 1.0);
                }
            }
            id
        }
        "medical" | "sick" => {
            let c = match field(cmd, "citizen") {
                Some(v) => key_from(v)?,
                None => {
                    let b: nd_sim::BuildingId = key_from(field(cmd, "building").ok_or("citizen or building is required")?)?;
                    w.citizens.iter().find(|(_, c)| c.inside == Some(b)).map(|(id, _)| id).ok_or("nobody is inside that building")?
                }
            };
            w.fall_ill(c).ok_or("that citizen is not indoors or is already a patient")?
        }
        // [emergency] A car chase: a free police car comes for a suspect
        // whose car is parked near home, and they run for it.
        "chase" => w.stage_chase()?,
        // [driving] A crash: the car given (or the nearest to `at`) crashes
        // at `speed` m/s (25: written off). Processed on the next tick.
        "crash" => {
            let car: nd_sim::CarId = match field(cmd, "car") {
                Some(v) => key_from(v)?,
                None => {
                    let at = cmd.get("at").and_then(|v| v.as_array()).and_then(|a| Some(nd_sim::Vec2::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))).unwrap_or(nd_sim::Vec2::ZERO);
                    w.traffic.cars.iter().filter(|(_, c)| !c.siren && c.bus.is_none() && !c.in_stunt() && !c.crashed()).min_by(|a, b| a.1.pos.distance(at).total_cmp(&b.1.pos.distance(at))).map(|(id, _)| id).ok_or("no car on the road")?
                }
            };
            if !w.traffic.cars.contains_key(car) {
                return Err("no such car".into());
            }
            let speed = cmd.get("speed").and_then(|v| v.as_f64()).unwrap_or(25.0) as f32;
            w.traffic.crash(vec![car], nd_sim::traffic::CrashCause::OffRoad, speed);
            return Ok(json!({ "car": key_str(car), "crash": w.traffic.next_crash - 1 }));
        }
        // [driving] A street takeover at the junction nearest `at`.
        "takeover" => {
            let at = cmd.get("at").and_then(|v| v.as_array()).and_then(|a| Some(nd_sim::Vec2::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32)));
            let id = w.start_takeover(at)?;
            return Ok(json!({ "takeover": id }));
        }
        // [moto] A moto pack meeting at the road end nearest `at`.
        "moto_pack" | "moto" => {
            let at = cmd.get("at").and_then(|v| v.as_array()).and_then(|a| Some(nd_sim::Vec2::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32)));
            let id = w.start_moto_pack(at)?;
            return Ok(json!({ "moto_pack": id }));
        }
        _ => return Err(format!("unknown incident kind {kind:?}; fire, crime, medical, crash, takeover or moto_pack")),
    };
    Ok(json!({ "incident": id }))
}

/// The incident record: `active` (only those under way), `limit` (newest
/// first, default 50), `kind` fire / crime / medical / crash.
fn incidents(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let only_active = opt_bool(cmd, "active")?.unwrap_or(false);
    let limit = opt_u64(cmd, "limit")?.unwrap_or(50) as usize;
    let kind = opt_str(cmd, "kind")?.map(|k| k.to_ascii_lowercase());
    let rows: Vec<Value> = w
        .services
        .incidents
        .iter()
        .rev()
        .filter(|i| !only_active || i.active())
        .filter(|i| kind.as_deref().is_none_or(|k| i.kind.label() == k))
        .take(limit)
        .map(|i| incident_row(w, i))
        .collect();
    let s = &w.services.stats;
    Ok(json!({
        "returned": rows.len(), "total_on_record": w.services.incidents.len(), "active": w.services.active().count(),
        "jail_occupancy": w.services.jail_occupancy().iter().map(|(b, n)| json!({ "jail": key_str(*b), "name": building_name(w, Some(*b)), "inmates": n })).collect::<Vec<_>>(),
        "held_at_stations": s.held_at_stations, "released_early_total": s.released_early_total,
        "out_of_town_calls_today": s.out_of_town_calls_today, "out_of_town_fees_today": round(s.out_of_town_fees_today),
        "chases_now": w.services.chases.len(), "chases_today": s.chases_today, "chases_tagged_today": s.chases_tagged_today, "chases_lost_today": s.chases_lost_today, "chases_total": s.chases_total, "chases_tagged_total": s.chases_tagged_total,
        "second_alarms_today": s.second_alarms_today, "second_alarms_total": s.second_alarms_total,
        "incidents": rows,
    }))
}

fn incident_row(w: &nd_sim::World, i: &nd_sim::Incident) -> Value {
    json!({
        "id": i.id, "kind": i.kind.label(), "title": i.title(), "stage": i.stage.label(), "outcome": i.outcome.map(|o| o.label()),
        "building": key_str(i.building), "place": i.place, "pos": v2(i.pos),
        "started": i.started, "called": i.reported, "dispatched": i.dispatched, "arrived": i.arrived, "resolved": i.resolved,
        "call_to_dispatch_min": i.dispatched.zip(i.reported).map(|(a, b)| a.saturating_sub(b)), "dispatch_to_scene_min": i.travel_ticks(),
        "on_scene_min": i.work_ticks(), "call_to_arrival_min": i.response_ticks(),
        "unit": i.unit_name, "out_of_town": i.outside, "fee": round(i.fee), "intensity": round(i.intensity),
        "second_alarm": i.second_alarm, "second_unit": i.second.and_then(|u| w.services.units.get(u)).map(|u| format!("{} {}", u.kind.call_sign(), u.number)), "engines_on_scene": (i.kind == nd_sim::IncidentKind::Fire).then(|| w.engines_on_scene(i.id)),
        "people": i.people.iter().map(|p| json!({ "citizen": key_str(p.citizen), "name": p.name, "role": p.role, "taken_to": p.taken_to })).collect::<Vec<_>>(),
        "damage": i.damage.iter().map(|d| json!({ "building": key_str(d.building), "name": d.name, "condition_lost": round(d.lost), "destroyed": d.destroyed })).collect::<Vec<_>>(),
        "still_burning": i.active() && i.kind == nd_sim::IncidentKind::Fire && w.services.is_burning(i.building),
        "chase": i.chase.as_ref().map(|c| json!({ "vehicle": c.vehicle, "started": c.started, "ended": c.ended, "result": c.result.map(|r| r.label()), "metres": round(c.metres), "closest_m": round(c.closest) })),
    })
}

// ── buses ───────────────────────────────────────────────────────────────────

fn bus(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let action = req_str(cmd, "action")?;
    let w = &mut ctx.sim.world;
    let route_arg = |w: &nd_sim::World| -> Result<nd_sim::RouteId, String> {
        let id = key_from(field(cmd, "route").ok_or("missing route id")?)?;
        if !w.transit.routes.contains_key(id) {
            return Err("no such route".into());
        }
        Ok(id)
    };
    match action {
        // Highlight a line on the map (its path, stops and buses), as
        // clicking it in City Data's Services, Transit does; no route clears it.
        "select" => {
            let id = match field(cmd, "route") {
                Some(_) => Some(route_arg(w)?),
                None => None,
            };
            ctx.tools.route_sel = id;
            Ok(json!({ "route": opt_key(id) }))
        }
        "new_route" => {
            let id = w.add_bus_route();
            ctx.tools.route_sel = Some(id);
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        "add_stop" => {
            let p = req_point(cmd)?;
            // No route given: extend the one being drawn, or start a new one.
            let id = match field(cmd, "route") {
                Some(_) => route_arg(w)?,
                None => match ctx.tools.route_sel.filter(|r| w.transit.routes.contains_key(*r)) {
                    Some(r) => r,
                    None => {
                        let r = w.add_bus_route();
                        ctx.tools.route_sel = Some(r);
                        r
                    }
                },
            };
            let idx = w.add_bus_stop(id, p)?;
            Ok(json!({ "route": key_str(id), "stop_index": idx, "row": route_row(id, &w.transit.routes[id]) }))
        }
        "move_stop" => {
            let id = route_arg(w)?;
            let idx = opt_u64(cmd, "index")?.ok_or("missing stop index")? as usize;
            let p = req_point(cmd)?;
            if !w.move_bus_stop(id, idx, p) {
                return Err("stop could not be moved there (no street within reach?)".into());
            }
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        "remove_stop" => {
            let id = route_arg(w)?;
            let idx = opt_u64(cmd, "index")?.ok_or("missing stop index")? as usize;
            if w.transit.routes[id].stops.len() <= idx {
                return Err("no such stop".into());
            }
            w.remove_bus_stop(id, idx);
            ctx.tools.stop_sel = None;
            let alive = w.transit.routes.contains_key(id);
            if !alive && ctx.tools.route_sel == Some(id) {
                ctx.tools.route_sel = None;
            }
            Ok(json!({ "route": key_str(id), "route_alive": alive, "row": w.transit.routes.get(id).map(|r| route_row(id, r)) }))
        }
        "set_buses" => {
            let id = route_arg(w)?;
            let n = opt_u64(cmd, "n")?.ok_or("missing n")?.min(20) as u8;
            w.set_route_buses(id, n);
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        "remove_route" => {
            let id = route_arg(w)?;
            w.remove_bus_route(id);
            if ctx.tools.route_sel == Some(id) {
                ctx.tools.route_sel = None;
            }
            Ok(json!({ "removed": key_str(id), "routes": w.transit.routes.len() }))
        }
        "list" => Ok(json!({ "routes": w.transit.routes.iter().map(|(id, r)| route_row(id, r)).collect::<Vec<_>>() })),
        // [lines] The Lines window's figures for one line.
        "line" => {
            let id = route_arg(w)?;
            Ok(view::line_detail(w, id))
        }
        "rename" => {
            let id = route_arg(w)?;
            w.rename_bus_route(id, req_str(cmd, "name")?)?;
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        "recolour" | "recolor" => {
            let id = route_arg(w)?;
            let c = opt_u64(cmd, "color")?.ok_or("missing color (0..7)")?;
            w.recolour_bus_route(id, (c % nd_sim::vehicle::PAINTS as u64) as u8)?;
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        // Mark a stop on the map as the Lines window's "show" does; no
        // route clears the mark. look: true also points the camera at it.
        "select_stop" => {
            if field(cmd, "route").is_none() {
                ctx.tools.stop_sel = None;
                return Ok(json!({ "stop": Value::Null }));
            }
            let id = route_arg(w)?;
            let idx = opt_u64(cmd, "index")?.ok_or("missing stop index")? as usize;
            let pos = w.transit.routes[id].stops.get(idx).ok_or("no such stop")?.pos;
            ctx.tools.route_sel = Some(id);
            ctx.tools.stop_sel = Some((id, idx));
            if cmd.get("look").and_then(Value::as_bool).unwrap_or(false) {
                for mut cam in ctx.cams.iter_mut() {
                    cam.target = bevy::prelude::Vec3::new(pos.x, 0.0, pos.y);
                    cam.distance = cam.distance.min(120.0);
                }
            }
            Ok(json!({ "route": key_str(id), "index": idx, "pos": [pos.x, pos.y] }))
        }
        _ => Err(format!("unknown bus action {action:?}; one of select, new_route, add_stop, move_stop, remove_stop, set_buses, remove_route, rename, recolour, line, select_stop, list")),
    }
}

// ── find / inspect / select ─────────────────────────────────────────────────

fn str_filter(filter: Option<&Value>, name: &str) -> Option<String> {
    filter.and_then(|f| f.get(name)).and_then(Value::as_str).map(|s| s.to_ascii_lowercase())
}

fn bool_filter(filter: Option<&Value>, name: &str) -> Option<bool> {
    filter.and_then(|f| f.get(name)).and_then(Value::as_bool)
}

fn find(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let what = req_str(cmd, "what")?;
    let near = opt_vec2(cmd, "near")?;
    let radius = opt_f32(cmd, "radius")?.unwrap_or(if near.is_some() { 100.0 } else { f32::INFINITY });
    let limit = opt_u64(cmd, "limit")?.unwrap_or(50) as usize;
    let filter = field(cmd, "filter");
    let w = &ctx.sim.world;
    // (distance, row) so results can be sorted nearest-first when asked.
    let mut rows: Vec<(f32, Value)> = Vec::new();
    let within = |p: SVec2| -> Option<f32> {
        let d = near.map_or(0.0, |n| n.distance(p));
        (d <= radius).then_some(d)
    };
    let contains = |hay: &str, needle: &Option<String>| needle.as_ref().is_none_or(|n| hay.to_ascii_lowercase().contains(n));
    match what {
        "citizens" | "citizen" | "people" => {
            let f_activity = str_filter(filter, "activity");
            let f_name = str_filter(filter, "name");
            let f_class = str_filter(filter, "class").or(str_filter(filter, "tier"));
            let f_visible = bool_filter(filter, "visible");
            let f_spot = bool_filter(filter, "spotlight");
            for (id, c) in w.citizens.iter() {
                if !contains(c.activity.label(), &f_activity) || !contains(&c.name, &f_name) || !contains(c.class().label(), &f_class) {
                    continue;
                }
                if f_visible.is_some_and(|v| v != c.is_visible()) || f_spot.is_some_and(|v| v != c.spotlight) {
                    continue;
                }
                if let Some(d) = within(c.pos) {
                    rows.push((d, citizen_row(w, id, c)));
                }
            }
        }
        "buildings" | "building" => {
            let f_lu = str_filter(filter, "land_use");
            let f_tier = str_filter(filter, "tier");
            let f_biz = str_filter(filter, "business_kind").or(str_filter(filter, "business"));
            let f_name = str_filter(filter, "name");
            let f_site = bool_filter(filter, "under_construction");
            let f_plant = bool_filter(filter, "plant");
            let f_rail = bool_filter(filter, "rail");
            let f_depot = bool_filter(filter, "depot");
            let f_derelict = bool_filter(filter, "derelict");
            for (id, b) in w.buildings.iter() {
                if !contains(b.land_use.label(), &f_lu) || !contains(b.tier.label(), &f_tier) || !contains(&b.name, &f_name) {
                    continue;
                }
                if f_biz.is_some() && !b.business.as_ref().is_some_and(|z| contains(z.kind.label(), &f_biz)) {
                    continue;
                }
                if f_site.is_some_and(|v| v != b.construction.is_some()) || f_plant.is_some_and(|v| v != b.plant.is_some()) || f_rail.is_some_and(|v| v != b.rail.is_some()) || f_depot.is_some_and(|v| v != b.depot) || f_derelict.is_some_and(|v| v != b.is_derelict()) {
                    continue;
                }
                let Some(lot) = w.lots.get(b.lot) else { continue };
                if let Some(d) = within(lot.center) {
                    rows.push((d, building_row(w, id, b)));
                }
            }
        }
        "lots" | "lot" => {
            let f_lu = str_filter(filter, "land_use");
            let f_vacant = bool_filter(filter, "vacant");
            let f_park = bool_filter(filter, "park");
            let min_area = filter.and_then(|f| f.get("min_area")).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            for (id, l) in w.lots.iter() {
                if !contains(l.land_use.label(), &f_lu) || l.area() < min_area {
                    continue;
                }
                if f_vacant.is_some_and(|v| v != (l.building.is_none() && l.usable())) || f_park.is_some_and(|v| v != l.park.is_some()) {
                    continue;
                }
                if let Some(d) = within(l.center) {
                    rows.push((d, lot_row(w, id, l)));
                }
            }
        }
        "cars" | "car" | "vehicles" => {
            let f_kind = str_filter(filter, "kind");
            for (id, c) in w.traffic.cars.iter() {
                if !contains(car_kind(w, c), &f_kind) {
                    continue;
                }
                if let Some(d) = within(c.pos) {
                    rows.push((d, car_row(w, id, c)));
                }
            }
        }
        "trains" | "train" => {
            let f_kind = str_filter(filter, "kind");
            for (id, t) in w.rail.trains.iter() {
                let row = train_row(w, id, t);
                if !contains(row["kind"].as_str().unwrap_or(""), &f_kind) {
                    continue;
                }
                let head = nd_sim::rail::point_along(&t.poly, t.s).0;
                if let Some(d) = within(head) {
                    rows.push((d, row));
                }
            }
        }
        // [train-inspect] Metro trains in their tunnels.
        "metro_trains" | "metro_train" | "metros" => {
            for (id, t) in w.subway.trains.iter() {
                if let Some(d) = within(t.head()) {
                    rows.push((d, trains::metro_row(w, id)));
                }
            }
        }
        // [camera] Pavement delivery robots; `filter.shop` matches the shop's name.
        "bots" | "bot" | "delivery_bots" => {
            let f_shop = str_filter(filter, "shop");
            for (id, b) in w.bots.bots.iter() {
                let shop = w.buildings.get(b.shop).map_or("", |s| s.name.as_str());
                if !contains(shop, &f_shop) {
                    continue;
                }
                if let Some(d) = within(b.pos) {
                    rows.push((d, follow::bot_row(w, id)));
                }
            }
        }
        "drones" | "drone" | "aircraft" => {
            let f_kind = str_filter(filter, "kind");
            for (id, d) in w.air.drones.iter() {
                if !contains(d.craft.label(), &f_kind) {
                    continue;
                }
                if let (Some(dist), Some(row)) = (within(d.pos), view::drone_detail(w, id)) {
                    rows.push((dist, row));
                }
            }
        }
        "routes" | "route" | "lines" => {
            for (id, r) in w.transit.routes.iter() {
                let d = near.map_or(0.0, |n| r.stops.iter().map(|s| s.pos.distance(n)).fold(f32::INFINITY, f32::min));
                if d <= radius {
                    rows.push((d, route_row(id, r)));
                }
            }
        }
        "parcels" | "parcel" => {
            let f_lu = str_filter(filter, "land_use");
            let f_zoned = bool_filter(filter, "zoned");
            let f_landlocked = bool_filter(filter, "landlocked");
            let planned = w.planned_zoning();
            for (i, p) in w.parcels.parcels.iter().enumerate() {
                let built = p.land_use.map_or("unzoned", |l| l.label());
                if !contains(built, &f_lu) || f_zoned.is_some_and(|z| z != p.land_use.is_some()) || f_landlocked.is_some_and(|l| l != p.landlocked()) {
                    continue;
                }
                if let Some(d) = within(p.center) {
                    let accepts: Vec<&str> = nd_sim::LandUse::ALL.iter().filter(|l| p.accepts(**l)).map(|l| l.label()).collect();
                    let plan = planned.get(i).copied().flatten().map_or("unzoned", |l| l.label());
                    rows.push((d, json!({
                        "index": i, "center": v2(p.center), "area": round(p.area), "land_use": built, "planned": plan,
                        "landlocked": p.landlocked(), "street": p.street_frontage, "rough": p.rough, "rough_why": p.rough_why, "accepts": accepts,
                        "lot": p.lot.map(key_str), "key": [p.key.center.0, p.key.center.1],
                    })));
                }
            }
        }
        "gestures" => {
            for (id, g) in &w.effective_plan().gestures {
                let d = near.map_or(0.0, |n| g.points().iter().map(|p| p.distance(n)).fold(f32::INFINITY, f32::min));
                if d <= radius {
                    rows.push((d, gesture(*id, g)));
                }
            }
        }
        other => return Err(format!("unknown what {other:?}; one of citizens, buildings, lots, parcels, cars, trains, metro_trains, routes, gestures")),
    }
    let total = rows.len();
    if near.is_some() {
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    let rows: Vec<Value> = rows
        .into_iter()
        .take(limit)
        .map(|(d, mut r)| {
            if near.is_some() {
                r["distance"] = json!(round(d));
            }
            r
        })
        .collect();
    Ok(json!({ "what": what, "total": total, "returned": rows.len(), "rows": rows }))
}

/// Set or reset the lane arrows of one street arriving at a junction.
fn lane_use(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let at = opt_vec2(cmd, "junction")?.ok_or("missing field \"junction\": [x, y]")?;
    let w = &mut ctx.sim.world;
    let k = view::junction_near(w, at).ok_or("no junctions")?;
    let node = w.traffic.net.intersections[k].node;
    let from = opt_vec2(cmd, "approach")?.ok_or("missing field \"approach\": [x, y] on the arriving street")?;
    let segment = w
        .traffic
        .net
        .approaches
        .iter()
        .filter(|a| a.node == node)
        .min_by(|a, b| {
            let d = |s| {
                let (p, q) = w.roads.segment_points(s);
                nd_sim::roads::project_on_segment(from, p, q).1.distance(from)
            };
            d(a.segment).total_cmp(&d(b.segment))
        })
        .map(|a| a.segment)
        .ok_or("no street arrives at that junction")?;
    let arrows = match field(cmd, "lanes") {
        Some(Value::String(s)) if s == "reset" || s == "default" => None,
        Some(Value::Array(a)) => Some(
            a.iter()
                .map(|x| x.as_str().and_then(nd_sim::traffic::parse_arrows).ok_or_else(|| format!("lane arrows like \"L\", \"S+R\", \"L+S\", got {x}")))
                .collect::<Result<Vec<u8>, String>>()?,
        ),
        _ => return Err("field \"lanes\": [\"L\", \"S+R\", ...] leftmost first, or \"reset\"".into()),
    };
    w.traffic.set_lane_use(segment, node, arrows)?;
    Ok(json!({ "junction": view::junction_detail(w, k) }))
}

fn inspect(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    // [toll-inspect] A toll booth by its id.
    if let Some(v) = field(cmd, "toll") {
        let id = tolls::toll_id(w, v)?;
        return Ok(json!({ "toll": tolls::toll_detail(w, id) }));
    }
    if let Some(v) = field(cmd, "citizen") {
        let id = key_from(v)?;
        let c = w.citizens.get(id).ok_or("no such citizen")?;
        let mut v = citizen_detail(w, id, c, ctx.tools.favorites.contains(&id));
        v["places"] = places::citizen_places(w, id);
        v["social"] = social::citizen_social(w, id);
        v["holdings"] = follow::holdings_json(w, id);
        return Ok(json!({ "citizen": v }));
    }
    if let Some(v) = field(cmd, "building") {
        let id = key_from(v)?;
        let b = w.buildings.get(id).ok_or("no such building")?;
        let mut v = building_detail(w, id, b, building_style(&ctx.sim, id, &ctx.assets));
        v["place"] = places::building_place(w, id);
        // [land] owner, sale price, plots, form, pressure
        v["land"] = property::building_land(w, id);
        // [supertall] height, rank in the skyline, and a league tower's record
        v["tower"] = skyline::building_tower(w, id);
        // [ferry-routes] a terminal's routes, a shipping port's lane
        if crate::ferry_select::is_route_dock(w, id) {
            v["routes"] = ferry::routes_json(w, id);
        }
        return Ok(json!({ "building": v }));
    }
    if let Some(v) = field(cmd, "lot") {
        let id = key_from(v)?;
        let l = w.lots.get(id).ok_or("no such lot")?;
        return Ok(json!({ "lot": lot_detail(w, id, l) }));
    }
    if let Some(v) = field(cmd, "car") {
        let id = key_from(v)?;
        let c = w.traffic.cars.get(id).ok_or("no such car")?;
        let mut v = json!({ "car": car_detail(w, id, c) });
        // [fire-scene] An emergency unit's car: the unit card as well.
        if let Some(uid) = w.services.unit_of_car(id) {
            v["unit"] = units::unit_detail(&ctx.sim, uid, &ctx.assets.rules).unwrap_or(Value::Null);
        }
        return Ok(v);
    }
    if let Some(v) = field(cmd, "unit") {
        let id = key_from(v)?;
        return Ok(json!({ "unit": units::unit_detail(&ctx.sim, id, &ctx.assets.rules).ok_or("no such unit")? }));
    }
    if let Some(v) = field(cmd, "vehicle") {
        let id = key_from(v)?;
        return Ok(json!({ "vehicle": view::vehicle_detail(w, id).ok_or("no such vehicle")? }));
    }
    if let Some(v) = field(cmd, "bot") {
        let id = key_from(v)?;
        return Ok(json!({ "bot": follow::bot_detail(w, id).ok_or("no such delivery bot")? }));
    }
    if let Some(v) = field(cmd, "junction") {
        let k = view::junction_near(w, vec2_of(v)?).ok_or("no junctions")?;
        return Ok(json!({ "junction": view::junction_detail(w, k) }));
    }
    if let Some(v) = field(cmd, "drone") {
        let id: nd_sim::air::DroneId = key_from(v)?;
        return Ok(json!({ "drone": view::drone_detail(w, id).ok_or("no such drone")? }));
    }
    if let Some(v) = field(cmd, "train") {
        let id = key_from(v)?;
        return Ok(json!({ "train": trains::train_detail(w, id, &ctx.assets.palette.car_colors).ok_or("no such train")? }));
    }
    if let Some(v) = field(cmd, "metro") {
        let id = key_from(v)?;
        return Ok(json!({ "metro": trains::metro_detail(w, id).ok_or("no such metro train")? }));
    }
    if let Some(v) = field(cmd, "ship") {
        let id = key_from(v)?;
        return Ok(json!({ "ship": ferry::ship_json(w, id).ok_or("no such ship")? }));
    }
    if let Some(v) = field(cmd, "route") {
        let id = key_from(v)?;
        let r = w.transit.routes.get(id).ok_or("no such route")?;
        return Ok(json!({ "route": route_row(id, r) }));
    }
    if let Some(v) = field(cmd, "gesture") {
        let id = GestureId(v.as_u64().ok_or("gesture id is a number")?);
        let g = w.gesture(id).ok_or("no such gesture")?;
        return Ok(json!({ "gesture": gesture(id, &g) }));
    }
    // A road piece: by id, or the one nearest a point.
    // [road-inspect] `road`: an id, or [x, y] picked as a click would be.
    let seg = match (field(cmd, "segment"), field(cmd, "road")) {
        (Some(v), _) => Some(nd_sim::SegmentId(v.as_u64().ok_or("segment id is a number")? as u32)),
        (None, Some(v)) => Some(road::road_id(w, v, ctx.tools.depth)?),
        _ => None,
    };
    if let Some(sid) = seg {
        let s = w.roads.try_segment(sid).ok_or("no such segment")?;
        let (a, b) = w.roads.segment_points(sid);
        let z = |n: nd_sim::NodeId| w.road_z.get(n.0 as usize).copied().unwrap_or(0.0);
        let level = if s.tunnel { "tunnel" } else if s.viaduct { "viaduct" } else if s.bridge { "bridge" } else if w.roads.node(s.a).level < 0.0 || w.roads.node(s.b).level < 0.0 { "cutting" } else { "ground" };
        let crossings: Vec<Value> = w
            .roads
            .grade_crossings()
            .into_iter()
            .filter(|c| c.upper.0 == sid || c.lower.0 == sid)
            .map(|c| {
                let other = if c.upper.0 == sid { c.lower.0 } else { c.upper.0 };
                json!({ "at": [round(c.at.x), round(c.at.y)], "passes": if c.upper.0 == sid { "over" } else { "under" }, "other_segment": other.0, "clearance": round((w.road_height(c.upper.0, c.upper.1) - w.road_height(c.lower.0, c.lower.1)).abs()) })
            })
            .collect();
        return Ok(json!({ "segment": {
            "id": sid.0, "from": [round(a.x), round(a.y)], "to": [round(b.x), round(b.y)], "kind": road_kind_name(s.kind),
            "lanes_fwd": s.lanes_fwd, "lanes_bwd": s.lanes_bwd, "highway": s.is_highway(),
            "level": level, "levels": [w.roads.node(s.a).level, w.roads.node(s.b).level], "heights": [round(z(s.a)), round(z(s.b))], "inner_heights": w.road_inner.get(sid.0 as usize).map(|v| v.iter().map(|(t, h)| [round(*t), round(*h)]).collect::<Vec<_>>()).unwrap_or_default(),
            "bridge": s.bridge, "viaduct": s.viaduct, "tunnel": s.tunnel,
            "nodes": [s.a.0, s.b.0], "junction_degrees": [w.roads.neighbors(s.a).count(), w.roads.neighbors(s.b).count()],
            "grade_crossings": crossings,
            "condition": nights::segment_condition(w, sid),
        }, "road": road::road_detail(w, sid) }));
    }
    // No id: whatever is selected.
    match *ctx.selection {
        Selection::Toll(id) => inspect(&json!({ "toll": id }), ctx),
        Selection::Road(sid) => inspect(&json!({ "segment": sid.0 }), ctx),
        Selection::Citizen(id) => inspect(&json!({ "citizen": key_str(id) }), ctx),
        Selection::Building(id) => inspect(&json!({ "building": key_str(id) }), ctx),
        Selection::Car(id) => inspect(&json!({ "car": key_str(id) }), ctx),
        Selection::Parked(id) => inspect(&json!({ "vehicle": key_str(id) }), ctx),
        Selection::Bot(id) => inspect(&json!({ "bot": key_str(id) }), ctx),
        Selection::Train(id) => inspect(&json!({ "train": key_str(id) }), ctx),
        Selection::Metro(id) => inspect(&json!({ "metro": key_str(id) }), ctx),
        Selection::Unit(id) => inspect(&json!({ "unit": key_str(id) }), ctx),
        Selection::Ship(id) => inspect(&json!({ "ship": key_str(id) }), ctx),
        Selection::None => Err("inspect needs one of citizen, building, lot, car, bot, train, metro, toll, road, segment, route, gesture (an id), or a selection".into()),
    }
}

/// `tick_time` for other modules (the flags).
pub fn tick_time_json(tick: u64) -> Value {
    tick_time(tick)
}

/// Kind, id and label of a selection (`null` for none).
pub fn selection_json_brief(w: &nd_sim::World, sel: Selection) -> Value {
    selection(w, sel)
}

/// The building style label, as the inspector shows it: the style the
/// renderer draws (`nd_procgen::building_style`, one decision for both).
pub fn building_style(sim: &crate::sim::Sim, id: nd_sim::BuildingId, assets: &GameAssets) -> &'static str {
    nd_procgen::building_style(&sim.world, id, &assets.rules, Some(&sim.styles)).map_or("unknown", |s| s.label())
}

/// Everything about what the player has selected, as `inspect` gives it,
/// in one reply: a car with why it stands, the jam in front of it, its trip
/// and lanes; a citizen (and the car they are driving); a building; a
/// parked car; plus the bus line picked in City Data's Services, Transit, if any.
pub fn selected_detail(sim: &crate::sim::Sim, sel: Selection, route_sel: Option<nd_sim::RouteId>, assets: &GameAssets, favourites: &[nd_sim::CitizenId]) -> Value {
    let w = &sim.world;
    let detail = match sel {
        Selection::None => Value::Null,
        Selection::Toll(id) => tolls::toll_detail(w, id).unwrap_or(Value::Null),
        Selection::Road(sid) => road::road_detail(w, sid).unwrap_or(Value::Null),
        Selection::Car(id) => w.traffic.cars.get(id).map_or(Value::Null, |c| car_detail(w, id, c)),
        Selection::Citizen(id) => match w.citizens.get(id) {
            Some(c) => {
                let mut v = citizen_detail(w, id, c, favourites.contains(&id));
                v["places"] = places::citizen_places(w, id);
                v["social"] = social::citizen_social(w, id);
                v["holdings"] = follow::holdings_json(w, id);
                if let nd_sim::Activity::Driving { car, .. } = c.activity {
                    if let Some(x) = w.traffic.cars.get(car) {
                        v["driving_car"] = car_detail(w, car, x);
                    }
                }
                v
            }
            None => Value::Null,
        },
        Selection::Building(id) => match w.buildings.get(id) {
            Some(b) => {
                let mut v = building_detail(w, id, b, building_style(sim, id, assets));
                v["place"] = places::building_place(w, id);
                v
            }
            None => Value::Null,
        },
        Selection::Parked(id) => view::vehicle_detail(w, id).unwrap_or(Value::Null),
        Selection::Bot(id) => follow::bot_detail(w, id).unwrap_or(Value::Null),
        Selection::Train(id) => trains::train_detail(w, id, &assets.palette.car_colors).unwrap_or(Value::Null),
        Selection::Metro(id) => trains::metro_detail(w, id).unwrap_or(Value::Null),
        Selection::Unit(id) => units::unit_detail(sim, id, &assets.rules).unwrap_or(Value::Null),
        Selection::Ship(id) => ferry::ship_json(w, id).unwrap_or(Value::Null),
    };
    let bus_line = route_sel.and_then(|r| w.transit.routes.get(r).map(|route| {
        let mut v = route_row(r, route);
        v["buses"] = json!(w.traffic.cars.iter().filter(|(_, c)| c.bus.as_ref().is_some_and(|b| b.route == r)).map(|(id, c)| car_row(w, id, c)).collect::<Vec<_>>());
        v
    }));
    let brief = selection(w, sel);
    json!({
        "kind": brief.get("kind").cloned().unwrap_or(Value::Null), "id": brief.get("id").cloned().unwrap_or(Value::Null),
        "selection": brief, "detail": detail, "bus_line": bus_line, "clock": tick_time(w.clock.tick),
    })
}

/// `flags {since?, clear?}`: the player's "Flag for Claude" bookmarks, each
/// with its snapshot and the selection's live detail now.
fn flags_cmd(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let since = opt_u64(cmd, "since")?.unwrap_or(0).min(u32::MAX as u64) as u32;
    let rows: Vec<Value> = ctx
        .claude
        .list
        .iter()
        .filter(|f| f.id > since)
        .map(|f| {
            let live = selected_detail(&ctx.sim, f.selection, None, &ctx.assets, &ctx.tools.favorites);
            let now = (!live["detail"].is_null()).then_some(live["detail"].clone());
            crate::flags::flag_json(f, now)
        })
        .collect();
    let total = ctx.claude.list.len();
    if opt_bool(cmd, "clear")?.unwrap_or(false) {
        ctx.claude.list.clear();
    }
    Ok(json!({ "flags": rows, "total": total, "file": absolute(crate::flags::FLAG_FILE) }))
}

/// [road-inspect] `repair_road {road?}`: "Repair now" on a piece (by id or
/// [x, y]; else the selected one): resurfaced at once, paid from the
/// treasury as road maintenance.
fn repair_road(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let sid = match field(cmd, "road").or_else(|| field(cmd, "segment")) {
        Some(v) => road::road_id(&ctx.sim.world, v, ctx.tools.depth)?,
        None => match *ctx.selection {
            Selection::Road(sid) => sid,
            _ => return Err("missing field \"road\": an id or [x, y] (or select a road first)".into()),
        },
    };
    let cost = ctx.sim.world.repair_road_now(sid)?;
    Ok(json!({ "repaired": sid.0, "cost": round(cost), "treasury": round(ctx.sim.world.economy.treasury), "road": road::road_detail(&ctx.sim.world, sid) }))
}

fn select(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let sel = if let Some(v) = field(cmd, "toll") {
        // [toll-inspect]
        Selection::Toll(tolls::toll_id(w, v)?)
    } else if let Some(v) = field(cmd, "road").or_else(|| field(cmd, "segment")) {
        // [road-inspect] A street piece by id, or the one under [x, y].
        Selection::Road(road::road_id(w, v, ctx.tools.depth)?)
    } else if let Some(v) = field(cmd, "citizen") {
        let id = key_from(v)?;
        if !w.citizens.contains_key(id) {
            return Err("no such citizen".into());
        }
        Selection::Citizen(id)
    } else if let Some(v) = field(cmd, "building") {
        let id = key_from(v)?;
        if !w.buildings.contains_key(id) {
            return Err("no such building".into());
        }
        Selection::Building(id)
    } else if let Some(v) = field(cmd, "car") {
        let id = key_from(v)?;
        if !w.traffic.cars.contains_key(id) {
            return Err("no such car".into());
        }
        Selection::Car(id)
    } else if let Some(v) = field(cmd, "vehicle") {
        let id = key_from(v)?;
        if !w.vehicles.cars.get(id).is_some_and(|c| c.spot().is_some()) {
            return Err("no such parked vehicle".into());
        }
        Selection::Parked(id)
    } else if let Some(v) = field(cmd, "bot") {
        let id = key_from(v)?;
        if !w.bots.bots.contains_key(id) {
            return Err("no such delivery bot".into());
        }
        Selection::Bot(id)
    } else if let Some(v) = field(cmd, "train") {
        let id = key_from(v)?;
        if !w.rail.trains.contains_key(id) {
            return Err("no such train".into());
        }
        Selection::Train(id)
    } else if let Some(v) = field(cmd, "metro") {
        let id = key_from(v)?;
        if !w.subway.trains.contains_key(id) {
            return Err("no such metro train".into());
        }
        Selection::Metro(id)
    } else if let Some(v) = field(cmd, "unit") {
        // [fire-scene] An emergency unit (`units` lists them), driving or on scene.
        let id = key_from(v)?;
        if crate::fire_scene::unit_pose(w, id, ctx.sim.frac()).is_none() {
            return Err(if w.services.units.contains_key(id) { "that unit is in its station".into() } else { "no such unit".into() });
        }
        Selection::Unit(id)
    } else if let Some(v) = field(cmd, "ship") {
        // [ferry-routes] A ferry or freighter (`landscape` lists the ships).
        let id = key_from(v)?;
        if !w.ports.ships.contains_key(id) {
            return Err("no such ship".into());
        }
        Selection::Ship(id)
    } else if let Some(p) = opt_vec2(cmd, "at")? {
        // Pick exactly as a click with the Inspect tool does.
        crate::tools::pick(&*ctx.sim, p, ctx.tools.depth)
    } else {
        Selection::None
    };
    *ctx.selection = sel;
    Ok(json!({ "selection": selection(w, sel) }))
}


// ── tool / camera ───────────────────────────────────────────────────────────

fn tool(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let t = &mut ctx.tools;
    if let Some(name) = opt_str(cmd, "name")? {
        let tool = tool_of(name)?;
        if tool != t.tool {
            t.tool = tool;
            t.points.clear();
            t.drag = None;
        }
    }
    if let Some(at) = view::opt_vec2(cmd, "ring_at")? {
        t.ring_pin = Some(at);
        t.ring_hover = Some(at);
    }
    if let Some(d) = opt_f32(cmd, "diameter")? {
        t.ring_radius = (d * 0.5).clamp(nd_sim::roads::RING_MIN_RADIUS, nd_sim::roads::RING_MAX_RADIUS);
    }
    if let Some(n) = opt_u64(cmd, "ring_lanes")? {
        t.ring_lanes = (n.min(u8::MAX as u64) as u8).clamp(1, nd_sim::roads::RING_MAX_LANES);
    }
    if let Some(preset) = opt_str(cmd, "preset").ok().flatten().filter(|p| p.eq_ignore_ascii_case("roundabout")) {
        let _ = preset;
        t.tool = Tool::Road;
        t.roundabout = true;
        t.interchange = false;
    } else if let Some(preset) = opt_str(cmd, "preset")? {
        if !t.pick_preset(preset) {
            return Err(format!("unknown preset {preset:?}; one of {}, Tunnel, Roundabout, Interchange", ROAD_PRESETS.iter().map(|p| p.0).collect::<Vec<_>>().join(", ")));
        }
        if preset.eq_ignore_ascii_case("interchange") {
            t.tool = Tool::Road;
        }
    }
    if let Some(e) = opt_f32(cmd, "elevation")? {
        t.elevation = e.clamp(crate::tools::ELEVATION_MIN, crate::tools::ELEVATION_MAX);
    }
    if let Some(d) = opt_str(cmd, "depth")? {
        t.depth = crate::tools::Depth::parse(d).ok_or_else(|| format!("unknown depth {d:?}; underground, ground or sky"))?;
    }
    // `draw`: the points of a road being drawn, as if clicked (for the
    // preview); `cursor`: where the pointer stands for the preview.
    if field(cmd, "draw").is_some() {
        let (pts, levels) = view::req_points_levels(cmd, "draw")?;
        t.tool = Tool::Road;
        t.roundabout = false;
        t.points.clear();
        t.point_levels.clear();
        for (p, l) in pts.into_iter().zip(levels) {
            let l = l.unwrap_or(t.elevation);
            t.push_point(p, l);
        }
    }
    if let Some(c) = view::opt_vec2(cmd, "cursor")? {
        t.cursor_pin = Some(c);
    }
    // [plot-hover] `hover`: the same pin, for the Buildings and Zone tools'
    // plot highlight (the reply's `hover` says what a click there would do).
    if let Some(c) = view::opt_vec2(cmd, "hover")? {
        t.cursor_pin = Some(c);
    }
    if let Some(kind) = opt_str(cmd, "kind")? {
        // `kind` means whatever the active tool chooses between.
        match t.tool {
            Tool::Road | Tool::Edit => t.kind = road_kind_of(kind)?,
            Tool::Buildings => {
                let tier = opt_str(cmd, "tier")?.and_then(nd_sim::services::parse_tier);
                let cat = nd_sim::BuildCategory::ALL.iter().copied().find(|c| c.key() == kind.to_ascii_lowercase());
                match cat {
                    Some(c) => {
                        t.build_cat = c;
                        if let Some(first) = c.items().first() {
                            t.build = *first;
                        }
                    }
                    None => t.pick_build(nd_sim::Placeable::parse(kind, tier).ok_or_else(|| format!("unknown building {kind:?}; a category (power, services, transit, rail, attractions) or a kind as for place"))?),
                }
                if let Some(tier) = tier {
                    t.build_tier = tier;
                }
            }
            Tool::Rail => {
                t.rail_kind = match kind.to_ascii_lowercase().as_str() {
                    "station" => RailFacility::Station,
                    "freight_yard" | "yard" => RailFacility::FreightYard,
                    "central_station" | "central" => RailFacility::Central,
                    _ => return Err(format!("unknown rail kind {kind:?}; station, central_station or freight_yard")),
                }
            }
            Tool::Transit => {
                t.transit_mode = match kind.to_ascii_lowercase().as_str() {
                    "route" | "lines" => TransitMode::Route,
                    "depot" => TransitMode::Depot,
                    "metro" | "subway" => TransitMode::Metro,
                    _ => return Err(format!("unknown transit mode {kind:?}; route, depot or metro")),
                };
                t.route_sel = None;
            }
            Tool::Zone => match ZoneMode::parse(kind) {
                Some(m) => t.zone_mode = m,
                None => t.land_use = land_use_of(kind)?,
            },
            Tool::District => {
                t.district.mode = match kind.to_ascii_lowercase().as_str() {
                    "brush" => crate::district_tool::DistrictMode::Brush,
                    "polygon" => crate::district_tool::DistrictMode::Polygon,
                    _ => return Err("district kind is brush or polygon".into()),
                };
            }
            Tool::Bulldoze => {
                t.cleanup = match kind.to_ascii_lowercase().as_str() {
                    "cleanup" | "clean_up" | "clean" => true,
                    "remove" | "bulldoze" => false,
                    _ => return Err("bulldoze kind is remove or cleanup".into()),
                }
            }
            Tool::Inspect => return Err("this tool has no kind".into()),
        }
    }
    if let Some(lu) = opt_str(cmd, "land_use")? {
        if lu.eq_ignore_ascii_case("none") || lu.eq_ignore_ascii_case("unzoned") {
            t.unzone = true;
        } else {
            t.land_use = land_use_of(lu)?;
            t.unzone = false;
        }
    }
    if let Some(m) = opt_str(cmd, "mode")? {
        t.zone_mode = ZoneMode::parse(m).ok_or_else(|| format!("unknown zone mode {m:?}; polygon, brush or marquee"))?;
        t.stroke = None;
    }
    if let Some(r) = opt_f32(cmd, "radius")? {
        if t.tool == Tool::Bulldoze {
            t.cleanup_radius = r.clamp(nd_sim::world::CLEANUP_MIN_RADIUS, nd_sim::world::CLEANUP_MAX_RADIUS);
        } else {
            t.brush_radius = r.clamp(BRUSH_MIN, BRUSH_MAX);
        }
    }
    if let Some(u) = opt_bool(cmd, "unzone")? {
        t.unzone = u;
    }
    if let Some(k) = opt_str(cmd, "industry")? {
        t.industry = nd_sim::world::INDUSTRY_CHOICES.iter().map(|(key, _)| *key).find(|key| key.eq_ignore_ascii_case(k.trim())).ok_or_else(|| format!("unknown industry {k:?}"))?;
    }
    if let Some(n) = opt_u64(cmd, "lanes_fwd")? {
        t.lanes_fwd = n.min(4) as u8;
    }
    if let Some(n) = opt_u64(cmd, "lanes_bwd")? {
        t.lanes_bwd = n.min(4) as u8;
    }
    if t.kind == RoadKind::Street && t.lanes_fwd + t.lanes_bwd == 0 {
        t.lanes_fwd = 1;
    }
    if let Some(c) = opt_bool(cmd, "curved")? {
        t.curved = c;
    }
    if let Some(s) = opt_bool(cmd, "show_routes")? {
        t.show_routes = s;
    }
    if let Some(v) = field(cmd, "route") {
        let id = key_from(v)?;
        if !ctx.sim.world.transit.routes.contains_key(id) {
            return Err("no such route".into());
        }
        t.route_sel = Some(id);
    }
    if let Some(g) = opt_u64(cmd, "selected_gesture")? {
        t.selected_gesture = Some(GestureId(g));
    }
    let mut v = json!({ "tool": tool_state(t) });
    if let Some(p) = t.cursor_pin {
        v["hover"] = match crate::render::hover_site(&ctx.sim.world, t, p) {
            Some(s) => json!({ "at": v2(p), "ok": s.verdict.is_ok(), "note": s.verdict.as_ref().ok(), "reason": s.verdict.as_ref().err(), "outline": view::points(&s.outline) }),
            None => Value::Null,
        };
    }
    Ok(v)
}

fn camera(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    // Any explicit pose ends a follow and a flight; `follow` below may
    // start a new one.
    ctx.follow.stop();
    ctx.fly.flight = None;
    if let Some(mode) = opt_str(cmd, "mode")? {
        *ctx.mode = match mode.to_ascii_lowercase().as_str() {
            "orbit" => CameraMode::Orbit,
            "walk" => CameraMode::Walk,
            _ => return Err(format!("unknown camera mode {mode:?}; orbit or walk")),
        };
    }
    let w = &ctx.sim.world;
    let mut follow: Option<Selection> = None;
    if let Some(f) = field(cmd, "follow") {
        // A citizen id, a car id, or an object {citizen|car|building: id}.
        let (kind, id) = match f {
            Value::Object(m) => m.iter().next().map(|(k, v)| (k.as_str(), v)).ok_or("follow: empty object")?,
            v => ("citizen", v),
        };
        follow = Some(match kind {
            "citizen" => {
                // A bare id may name a citizen or a car; try both.
                let cid: nd_sim::CitizenId = key_from(id)?;
                let car: nd_sim::CarId = key_from(id)?;
                if w.citizens.contains_key(cid) {
                    Selection::Citizen(cid)
                } else if w.traffic.cars.contains_key(car) {
                    Selection::Car(car)
                } else {
                    return Err("follow: no such citizen or car".into());
                }
            }
            "car" => {
                let id = key_from(id)?;
                if !w.traffic.cars.contains_key(id) {
                    return Err("follow: no such car".into());
                }
                Selection::Car(id)
            }
            "building" => {
                let id = key_from(id)?;
                if !w.buildings.contains_key(id) {
                    return Err("follow: no such building".into());
                }
                Selection::Building(id)
            }
            "bot" => {
                let id = key_from(id)?;
                if !w.bots.bots.contains_key(id) {
                    return Err("follow: no such delivery bot".into());
                }
                Selection::Bot(id)
            }
            // [fire-scene]
            "unit" => {
                let id = key_from(id)?;
                if crate::fire_scene::unit_pose(w, id, ctx.sim.frac()).is_none() {
                    return Err("follow: no such unit out of its station".into());
                }
                Selection::Unit(id)
            }
            // [train-inspect]
            "train" => {
                let id = key_from(id)?;
                if !w.rail.trains.contains_key(id) {
                    return Err("follow: no such train".into());
                }
                Selection::Train(id)
            }
            "metro" => {
                let id = key_from(id)?;
                if !w.subway.trains.contains_key(id) {
                    return Err("follow: no such metro train".into());
                }
                Selection::Metro(id)
            }
            // [ferry-routes]
            "ship" => {
                let id = key_from(id)?;
                if !w.ports.ships.contains_key(id) {
                    return Err("follow: no such ship".into());
                }
                Selection::Ship(id)
            }
            _ => return Err("follow: one of citizen, car, bot, building, train, metro, ship".into()),
        });
    }
    let look_at = opt_vec2(cmd, "look_at")?;
    let x = opt_f32(cmd, "x")?;
    let z = opt_f32(cmd, "z")?.or(opt_f32(cmd, "y")?);
    let distance = opt_f32(cmd, "distance")?;
    let yaw = opt_f32(cmd, "yaw")?;
    let pitch = opt_f32(cmd, "pitch")?;
    let mut applied = false;
    for mut cam in &mut ctx.cams {
        if let Some(p) = look_at {
            cam.target = Vec3::new(p.x, 0.0, p.y);
        }
        if let Some(x) = x {
            cam.target.x = x;
        }
        if let Some(z) = z {
            cam.target.z = z;
        }
        if let Some(d) = distance {
            cam.distance = d.clamp(12.0, 2500.0);
        }
        if let Some(y) = yaw {
            cam.yaw = y;
        }
        if let Some(p) = pitch {
            cam.pitch = p.clamp(0.08, 1.5);
        }
        if let Some(sel) = follow {
            // Start on the thing right away rather than one frame later.
            let p = crate::camera::thing_pose(&ctx.sim, sel).map(|t| t.pos);
            if let Some(p) = p {
                cam.target = Vec3::new(p.x, 0.0, p.y);
                if distance.is_none() {
                    cam.distance = cam.distance.min(90.0);
                }
            }
        }
        applied = true;
    }
    if !applied {
        return Err("no camera yet".into());
    }
    // Walk mode: `look_at`/x/z place the eye; yaw/pitch turn the head.
    if *ctx.mode == CameraMode::Walk {
        let walk = &mut ctx.walk;
        if let Some(p) = look_at {
            walk.pos.x = p.x;
            walk.pos.z = p.y;
        }
        if let Some(x) = x {
            walk.pos.x = x;
        }
        if let Some(z) = z {
            walk.pos.z = z;
        }
        if let Some(y) = yaw {
            walk.yaw = y;
        }
        if let Some(p) = pitch {
            walk.pitch = p.clamp(-1.35, 1.35);
        }
    }
    if let Some(sel) = follow {
        let generation = ctx.sim.generation;
        ctx.follow.start(sel, generation);
    }
    Ok(json!({ "camera": camera_pose(ctx), "following": follow.map(|s| selection(&ctx.sim.world, s)) }))
}

// ── screenshot ──────────────────────────────────────────────────────────────

/// [look] The graphics setting, the tier in force (the setting's or Auto's
/// pick, which takes effect next frame), the frame time and the effects.
fn graphics_json(ctx: &Ctx) -> Value {
    let g = &ctx.stats.graphics;
    let tier = match ctx.lighting.quality {
        crate::render::QualitySetting::Fixed(q) => q,
        crate::render::QualitySetting::Auto => g.tier,
    };
    json!({
        "quality": ctx.lighting.quality.label().to_lowercase(), "tier": tier.label().to_lowercase(),
        "frame_ms": round(g.frame_ms), "sim_ms": round(g.sim_ms), "auto_steps": g.steps, "reason": g.reason, "effects": g.effects,
    })
}

fn screenshot(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let default = format!("screenshots/shot-{secs}.png");
    // PNG only: Bevy's `jpeg` image feature is not enabled in this build,
    // and the failure would surface only as a log line, not as a reply.
    let path = allowed_path(opt_str(cmd, "path")?.unwrap_or(&default), "png", true)?;
    // The capture runs in the render graph and the file is written by the
    // observer when the GPU copy lands, a frame or two from now; the reply
    // only promises the path.
    ctx.commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
    Ok(json!({ "path": path.to_string_lossy().replace('\\', "/"), "note": "written a frame or two later; poll for the file" }))
}

// ── review: export ──────────────────────────────────────────────────────────

/// Milliseconds since `start`, to two decimals.
fn ms(start: std::time::Instant) -> f64 {
    (start.elapsed().as_secs_f64() * 100_000.0).round() / 100.0
}

/// An optional array of strings, e.g. `sections`, `keys`, `fields`.
fn opt_strings(cmd: &Value, name: &str) -> Result<Option<Vec<String>>, String> {
    match field(cmd, name) {
        None => Ok(None),
        Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("{name:?} must be an array of strings"))).collect::<Result<Vec<_>, _>>().map(Some),
        Some(Value::String(s)) => Ok(Some(vec![s.clone()])),
        Some(_) => Err(format!("{name:?} must be an array of strings")),
    }
}

/// `export {path?}`: everything the review commands know, in one JSON file
/// for offline analysis: the full report, the summary map, history, and
/// every building, business, citizen, route, plant and rail facility as
/// `list` rows. Default `exports/city-<unix>.json` under the working
/// directory (the repo root under `cargo run`; `/exports` is git-ignored).
fn export(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let start = std::time::Instant::now();
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let default = format!("exports/city-{secs}.json");
    let path = allowed_path(opt_str(cmd, "path")?.unwrap_or(&default), "json", true)?;
    let w = &ctx.sim.world;
    let mut tables = serde_json::Map::new();
    for what in list::WHATS {
        tables.insert(what.to_string(), json!(list::rows(w, what)?));
    }
    let doc = json!({
        "format": "district-2040 city export v1",
        "exported_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        "clock": clock(&ctx.sim),
        "report": report::report(w, &[])?,
        "map_summary": map::map(w, None, "summary").unwrap_or(Value::Null),
        "history": ctx.history.to_json(None, None)?,
        "tables": tables,
    });
    let text = serde_json::to_string(&doc).map_err(|e| e.to_string())?;
    crate::sim::write_atomic(&path, text.as_bytes()).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(json!({ "path": path.to_string_lossy().replace('\\', "/"), "bytes": text.len(), "took_ms": ms(start) }))
}

// ── help ────────────────────────────────────────────────────────────────────

/// [build-highlight] `build_highlight {category, off}`: open the Build
/// window on a section, sub-section or kind so its buildings and service
/// areas show on the map; with no fields, what shows now.
fn build_highlight(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    use crate::ui::build;
    let now = |ctx: &Ctx| {
        let sel = ctx.highlight.sel;
        let view = ctx.highlight.view.as_ref();
        json!({
            "on": sel.is_some(),
            "section": sel.map(|s| s.cat.key()),
            "sub": sel.and_then(|s| s.sub),
            "focus": sel.and_then(|s| s.focus).map(|p| p.label()),
            "buildings": view.map_or(Vec::new(), |v| v.marked.iter().map(|m| json!({ "building": key_str(m.id), "name": m.name, "kind": format!("{:?}", m.family).to_lowercase(), "level": format!("{:?}", m.level).to_lowercase(), "label": m.label })).collect()),
            "summary": view.map(|v| v.summary.headline.clone()),
            "detail": view.map(|v| v.summary.detail.clone()),
            "homes_covered": view.and_then(|v| v.homes_covered),
            "uncovered": view.map(|v| v.summary.spots.iter().map(|s| json!({ "name": s.name, "at": [s.at.x, s.at.y], "homes": s.homes })).collect::<Vec<_>>()),
        })
    };
    if opt_bool(cmd, "off")? == Some(true) {
        ctx.windows.build = Some((false, None));
        if build::is_build_tool(ctx.tools.tool) {
            ctx.tools.tool = Tool::Inspect;
            ctx.tools.points.clear();
            ctx.tools.drag = None;
        }
        return Ok(json!({ "on": false }));
    }
    let Some(name) = opt_str(cmd, "category")?.or(opt_str(cmd, "item")?) else { return Ok(now(ctx)) };
    let (cat, sub, item) = build::resolve_highlight(name).ok_or_else(|| format!("nothing to highlight for {name:?}: a section (power, services, transit, rail, water, attractions), a sub-section (safety, sanitation, parking, metro...) or a kind (health, police, fire, school, metro_station...)"))?;
    if build::highlight_kinds(cat, sub).is_empty() {
        return Err(format!("{} has no buildings to highlight (roads, zones, districts, lines and tolls have none)", sub.unwrap_or(cat.label())));
    }
    if let Some(item) = item {
        build::activate(item, &mut ctx.tools);
    } else if build::is_build_tool(ctx.tools.tool) && build::current_item(&ctx.tools).and_then(|i| i.placeable()).is_none_or(|p| !build::highlight_kinds(cat, sub).contains(&build::norm(p))) {
        // A tool for something else would keep the window elsewhere.
        ctx.tools.tool = Tool::Inspect;
    }
    ctx.windows.build_sub = Some((cat, sub));
    Ok(json!({ "on": true, "section": cat.key(), "sub": sub, "item": item.map(|i| i.label()), "note": "the highlight and its summary are worked out next frame: build_highlight {} reads them" }))
}

fn help() -> Value {
    let mut v = json!({
        "transport": "POST http://127.0.0.1:<port>/cmd (default port 4817) with JSON {\"cmd\": name, ...fields} and Content-Type: application/json (or the header X-ND-Remote: 1); GET /status, GET /help. Replies {ok: true, ...} or {ok: false, error} with HTTP 400. Requests from web pages (an Origin or Sec-Fetch-Site header from another site, a foreign Host) get 403; bodies over 1 MB 413; a 504 means the command was withdrawn unrun. See docs/REMOTE.md.",
        "start_here": "to review the city: status -> report (all sections, ranked alerts) -> map {layer: hunger|land_value|...} for where -> history for trends -> list for tables -> inspect ids",
        "ids": "slotmap ids are \"idx:version\" strings (citizen, building, lot, car, train, route); gesture ids are plain integers. Positions are [x, y] sim metres.",
        "commands": {
            "help": {},
            "status": {},
            "pause": { "paused": "bool, omitted toggles" },
            "speed": { "index": "0..5", "value": "multiple of real time for what moves (1 = people walk at their true pace; the clock runs 15x faster than that), snapped to the nearest step" },
            "step": { "ticks": "advance n sim minutes now, even while paused (default 1, max 100000)" },
            "seed_demo": {},
            "save": { "name": "optional: save as a named game, saves/<name>.ron (file-safe) with a sidecar <name>.meta.json and a map picture <name>.png", "path": "optional; neither: writes saves/city.ron and a dated slot (with a picture)" },
            "load": { "path": "a .ron save (see files)" },
            "list_saves": { "note": "newest first: path, name, kind (named|latest|autosave|dated), modified_unix, bytes, day, population, treasury, buildings, landscape (null for saves without a sidecar), thumbnail (absolute path of the PNG, or null), city (\"Unnamed city\" when none), game_id; by_city groups the paths by city name" },
            "plan": {},
            "add_roundabout": { "center": "[x,y] (snaps onto a junction within 12 m or a street within 6 m)", "radius": "m to the ring centreline, 12..120 (or diameter 24..240)", "lanes": "1|2|3 ring lanes", "implement": "bool", "returns": "gesture_id, roundabout {joins, cuts_roads, takes_buildings}, project state" },
            "add_road": { "points": "[[x,y] or [x,y,level],...] (2+; level metres above ground, negative below)", "elevation": "level for points without one (number), or a list", "tunnel": "bool: default level -8 m", "lanes_fwd": "0..4 (streets)", "lanes_bwd": "0..4", "kind": "street|path|rail", "curved": "bool", "snap": "bool, default true (a path end also snaps onto a park gate)", "from_junction": "path only: start on this grounds_junctions index", "to_junction": "path only: end on this index", "implement": "bool, default false (pending in the project)", "returns": "gesture_id, joined (path ends on a grounds gate), project state" },
            "add_zone": { "polygon": "[[x,y],...] (3+)", "land_use": "residential|commercial|industrial|entertainment|civic|park", "implement": "bool" },
            "bulldoze": { "gesture_id": "int", "implement": "bool" },
            "project": { "action": "start|implement|undo|redo|discard" },
            "place": { "kind": "toll_booth|toll_plaza (on a road: across the carriageway)|station|freight_yard|depot|parking|coal|solar|wind|nuclear|hydro|tidal|geothermal|shipping_port|ferry_terminal|police|fire|hospital|clinic|school|dispatch|monument|stadium|jail|trash_depot (services: tier or kind:tier)", "tier": "low|mid|high", "x": "f", "y": "f", "note": "hydro beside a river or stream with a current >= 0.8 m/s, tidal on the sea shore, geothermal over a hot spot; docks at the water's edge (they build a pier and a street stub)" },
            "new_game": { "preset": "river_valley|islands|coast|delta|plains|flat (default river_valley)", "seed": "u64", "water": "0..1", "relief": "0..1 height variation", "branching": "0..1", "islands": "0..8", "climate": "temperate|desert|tropical|arctic|volcanic (default desert; iceland = volcanic)", "highway": "ignored: every new game has the highway and the trunk line", "note": "throws the city away; omitted levers take the preset's" },
            "terrain_at": { "x": "f", "y": "f", "returns": "natural/ground/surface heights, slope, water (kind, current, width), nearest shore, hot spot, power sites and dock placement for the lot there, survey (every land layer's value 0..1 there: resources, and the chance of each industry kind)" },
            "grant": { "amount": "money added to the treasury (dev), default 100000" },
            "landscape": { "returns": "preset and levers, rivers (width, current, hydro_ok), lakes, sea, hot spots, highway ends and through cars, bridges, docks, ships" },
            "incident": { "kind": "fire|crime|medical|crash|takeover|chase", "building": "id (fire, crime; medical without citizen)", "intensity": "fire, dev: start it this big (0..1; one engine knocks off 0.012 a minute)", "citizen": "id (medical)", "car": "id (crash; default the car nearest at)", "at": "[x,y] (crash, takeover: the junction nearest)", "speed": "m/s (crash, default 25 = written off)" },
            "incidents": { "active": "bool (only those under way)", "limit": "default 50, newest first", "kind": "fire|crime|medical|crash" },
            "bus": { "action": "select|new_route|add_stop|move_stop|remove_stop|set_buses|remove_route|rename|recolour|line|select_stop|list", "route": "id", "x": "f", "y": "f", "index": "stop index", "n": "buses", "name": "rename", "color": "0..7", "look": "select_stop: point the camera" },
            "find": { "what": "citizens|buildings|lots|parcels|cars|trains|drones|routes|gestures", "near": "[x,y]", "radius": "m (default 100 with near)", "filter": "object; citizens: activity,name,class,visible,spotlight; buildings: land_use,tier,business_kind,name,under_construction,plant,rail,depot,derelict; lots: land_use,vacant,park,min_area; cars/trains: kind", "limit": "default 50" },
            "inspect": { "citizen|building|lot|car|train|drone|vehicle|route": "id (vehicle: a private car's record; a unit's car also gets unit)", "unit": "id: an emergency unit: title, kind, station, state, doing, crew (citizen, name, role, out, pos, pose), incident (fire: how_bad, intensity, minutes_to_out, progress, damage; else patient, suspect, people), and for a fire engine at work scene (seconds, water_litres, aim, ladder {yaw_deg, pitch_deg, length, outriggers, tip}, hoses, streams)", "gesture": "int; with no id, the current selection", "junction": "[x,y]: the nearest junction's signal, lane arrows per arriving street and left turn waits", "segment": "id: a road piece: level (ground, viaduct, cutting, tunnel, bridge), levels, heights, flags, the ways it passes over or under", "road": "id, or [x,y]: the piece a click there picks (else the nearest), as segment, plus road: the road card (kind, lanes, limit, level, run, now {vehicles by kind, bikes, walkers, speed, congestion, queue}, flow, hourly (72 h), decay {wear, condition, effects, wear_per_day, next_condition, repair_cost, queue_place, last_repaired_day}, tolls, crossings, bus_stops, kerb parking)", "toll": "int: a toll booth in full (traffic, lorries, free, avoided, queue, hourly)", "ship": "id (landscape.ships): a ferry or freighter: title, doing, passengers, cargo, route; a ferry terminal or shipping port building also gets routes (to, ships, headway_min, next_departure, passengers_today)" },
            "lane_use": { "junction": "[x,y]", "approach": "[x,y] on the arriving street", "lanes": "[\"L\", \"S+R\", ...] leftmost first (L, S, R joined with +), or \"reset\" for the demand-chosen default" },
            "select": { "citizen|building|car|vehicle|bot": "id (vehicle: a parked car; bot: a delivery robot)", "unit": "id: an emergency unit (fire engine, police car, ambulance) driving or standing on scene (see units); a click on one picks it the same way", "toll": "int: a toll booth", "road|segment": "id or [x,y]: a street piece", "ship": "id: a ferry or freighter (landscape.ships)", "at": "[x,y] picks like a click (vehicles, people, booths, parked cars, then the road piece, then lots); nothing clears" },
            "favourite": { "citizen": "id", "on": "bool, omitted toggles" },
            "camera": { "look_at": "[x,y]", "x": "", "z": "", "distance": "12..2500", "yaw": "rad", "pitch": "rad", "follow": "citizen/car id or {citizen|car|bot|building: id}; any camera command ends a follow", "mode": "orbit|walk" },
            "overlay": { "active": "the map layer to show, or none (layers are exclusive: showing one hides the others)", "layer": "zoning|land_value|districts|traffic|traffic_types|paths|people|pollution, a land survey layer (farmland|oil|geothermal|rock|sand|water|shipping|best_industry|best_farming|best_oil|best_refinery|best_manufacturing|best_logistics|best_fishing|best_tech), or projects (the dim other-projects switch, not a layer)", "on": "bool, omitted toggles; on hides every other layer", "land": "a land survey layer key, or none", "<layer name>": "bool (older form): true shows it alone, false hides it if it shows" },
            "lighting": { "follow_clock": "bool", "hour": "0..24 (also unpins from the clock)" },
            "heat_vision": { "on": "bool, omitted toggles: the drone thermal camera view (also key H)" },
            "screenshot": { "path": "optional .png (see files); default screenshots/shot-<unix>.png; written a frame or two later" },
            "events": { "since": "seq", "limit": "default 500" },
            "thoughts": { "limit": "default 50" }
        }
    });
    // Kept out of the literal above: it is at the json! recursion limit.
    v["files"] = json!("save, load, export and screenshot paths must lie inside saves/, screenshots/ or exports/ under the game's working directory, or the OS temp folder; no '..'; extensions .ron, .json, .png");
    v["aliases"] = json!({ "tax": "taxes", "toll": "tolls", "districts": "district", "tourists": "tourism", "metro": "subway", "heat": "heat_vision", "favorite": "favourite", "quality": "graphics" });
    // [road-inspect]
    // [committee]
    v["commands"]["committee"] = json!({ "action": "state (default)|members|appoint {id, seat?}|dismiss {id}|alerts {limit, mark_seen}|check_alerts|projects|implement {project}|dismiss_project {project}|pick_site {project, option}|refresh_sites {project}|report|minutes {meeting?}|settings {backend: mock|local|claude, local_url, local_model, claude_model, claude_effort, claude_fallbacks, timeout_s, auto_meetings, veto_allowed, save, test}", "note": "the mayor's AI committee (docs/AI_COMMITTEE.md): fictional politicians, six seats, alerts, projects; never takes or returns an API key" });
    v["commands"]["grounds_junctions"] = json!({ "lot": "optional lot id", "kind": "park|plaza|stadium|campus|station|beach", "near": "[x,y]", "radius": "m around near (default 150)", "returns": "junctions [{index, pos, kind, lot, attached, walk_node, gesture_id}] (the Path tool's snap rings) and links (paths joined to grounds)" });
    v["commands"]["diary"] = json!({ "citizen": "id", "refresh": "bool: write it again", "returns": "day, text, pending (a model still writing: call again), by_model, backend; display only, never in the save" });
    v["commands"]["voices"] = json!({ "limit": "default 20", "returns": "conversations of spotlit or favourite citizens that got a model's line (template and line), lines_written, diaries_written, backend" });
    v["commands"]["committee_meeting"] = json!({ "start": "bool: call a meeting now (pauses the sim)", "vote": "{proposal, vote: yes|no|abstain|veto} (the mayor's vote)", "abstain_rest": "bool", "fast_forward": "bool: show all; with the stand-in, run to the mayor's vote or the end now", "transcript_from": "line index", "returns": "the meeting in session (stage, transcript, proposals with votes and tally) or the last minutes" });
    // [fire-scene]
    v["commands"]["units"] = json!({ "kind": "optional: fire_engine|police_car|ambulance (or Engine|Car|Medic)", "out": "bool: only units out of the station", "returns": "units: unit id, title, kind, state, doing, pos, incident; select {unit} / inspect {unit} take the id" });
    v["commands"]["repair_road"] = json!({ "road": "id or [x,y]; default the selected road", "returns": "repaired, cost, treasury, road (the card); pays the crews' price (lane-m x wear x 6) from the treasury as road maintenance" });
    v["commands"]["add_interchange"] = json!({ "from": "[x,y]: one end of the cross street, on one side of the highway", "to": "[x,y]: the other end, across the highway", "lanes_fwd": "cross street lanes (default 2)", "lanes_bwd": "(default 2)", "check": "bool: only plan it, add nothing", "implement": "bool", "returns": "gesture_ids, interchange {kept_street, terminals, crossings, roads [{points, lanes}]}, or refused {why, at}; project state" });
    v["commands"]["rebuild_roads"] = json!({ "returns": "segments_before, segments, roads_version", "note": "dev: lays the roads, lots and lanes again from the plan as an implement does, with the current road-building rules (nothing in the plan changes)" });
    // [plant-groups]
    // [dirty-industry]
    v["commands"]["remediate"] = json!({ "x": "f", "y": "f", "radius": "m, default 100, at most 300", "note": "pay to dig out the contamination there at once (450 per cell of full contamination)" });
    v["commands"]["cleanup"] = json!({ "x": "f", "y": "f", "radius": "m, default 100, at most 300", "quote": "bool: only price it", "note": "the Bulldoze tool's Clean up ground: paid up front (450 per cell of full contamination), a crew digs it out from the middle over 2-6 days; returns the quote and the crews at work. tool {tool: bulldoze, kind: cleanup, radius, hover: [x, y]} shows the preview" });
    v["commands"]["contaminate"] = json!({ "x": "f", "y": "f", "radius": "m, default 60", "amount": "0..1, default 0.6", "note": "testing: pour contamination onto the ground" });
    v["commands"]["paint_mural"] = json!({ "building": "id", "group": "crew id (default: the first street-art crew)", "quality": "-1..1, default 0.6", "seed": "u64", "note": "testing: a mural on that building's street face now" });
    v["commands"]["dirt_at"] = json!({ "x": "f", "y": "f", "returns": "contamination, dirty_nearby {plants [{building, name, what, distance_m, level, reach_m}], home_exposure, farm_exposure}" });
    v["commands"]["plant_preview"] = json!({ "kind": "solar|wind|coal|...", "x": "f", "y": "f", "note": "read-only: MW, cost and upkeep on the site a click there takes (a lot, or unzoned ground joined into one), the Sunfield / Wind Park it would join and what it adds, and any problem" });
    // [grid-weather]
    v["commands"]["resource"] = json!({ "x": "f", "y": "f (or z)", "returns": "wind (site_factor, mean_ms, now_ms, typical_yield, yield_now, cut_out_now) and solar (site_factor, climate_sun, sun, yield_now) at the point, and the climate" });
    v["commands"]["grid"] = json!({ "returns": "time-of-use import/export price now, demand now vs the full draw, weather, day_ahead (24 h: hour, temp_c, demand_mw, import_price, export_price if the weather holds), last_24h (recorded demand, supply, imports, exports, price, temperature)" });
    v["commands"]["weather"] = json!({ "returns": "label, cloud, wind (0..1), wind_ms, temp_c, spell (normal|heat wave|cold snap|storm), spell_hours_left, climate, turbine cut-in/rated/cut-out m/s" });
    // [camera]
    v["commands"]["follow"] = json!({ "citizen|car|bot|building|vehicle": "id, or the selection", "stop": "bool: end the follow (also on: false)", "pov": "bool: look through their eyes (Walk view) or back to orbit", "fly": "bool, default true: fly in smoothly first", "note": "the camera stays on it (pan locked, rotate and zoom free) until stopped, Esc, or it is gone" });
    v["commands"]["zoom_to"] = json!({ "citizen|car|bot|building|vehicle": "id, or the selection", "at": "[x,y] instead of a thing", "distance": "m (default by kind)", "pitch": "rad (default 0.62)", "note": "a smooth move of 0.7..2.2 s; replies at once" });
    v["commands"]["gift"] = json!({ "citizen": "id, or the selected citizen", "amount": "default 1000", "note": "from the treasury to their household (booked as gifts to residents)" });
    // [identity]
    v["commands"]["city_name"] = json!({ "name": "optional: rename the city (tidied, at most 40 characters; empty leaves it unnamed)", "returns": "name, display_name, game_id (null for a legacy save until it is next saved), history_entries" });
    v["commands"]["game_history"] = json!({ "action": "list (default)|bookmark|unmark|mark|revert", "game_id": "list: another game's history", "marked_only": "list: bool", "path": "bookmark/unmark/revert: an entry's path from list (or file: its file name)", "label": "bookmark/mark: the moment's name", "confirm": "revert: must be true (the running city is replaced, as Load)", "returns": "entries newest first: path, file, day, population, treasury, buildings, saved_unix, city, bookmark, thumbnail" });
    v["commands"]["graphics"] = json!({ "quality": "auto|low|medium|high: pin the graphics tier (pin one before screenshots that must compare) or hand it back to the frame-rate throttler; omitted reports the setting, the tier in force, the smoothed frame time and the effects on the camera" });
    // The review commands, added separately: one `json!` this size hits the
    // macro recursion limit.
    let review = json!({
        "report": { "sections": "optional subset of overview, people, economy, land, services, industry, society, alerts, history (default all but history): the one-call city review", "history_days": "days of the history section (default 30, max 365)" },
        "map": { "cell_m": "cell size in metres, default 100 (grown to keep at most 400 cells)", "layer": "summary (table of cells) | population | land_value | jobs | hunger | vacancy | power | traffic | fertility | oil | contamination | industry | crime | homeless | trash (grid + ASCII heat map)" },
        "industry": { "lot": "lot id, or", "polygon": "[[x, y], ...] area, or", "pos": "[x, y] (query only)", "kind": "farming | oil | refinery | manufacturing | logistics | fishing | tech | mining | smelting | factory_farming | none: sets the industry sub-zone bias (none hands the area back to the ground); omit to query the ground, contamination and industry weights of the lot" },
        "history": { "keys": "optional daily/hourly series names (default all)", "days": "last n days (default all kept, up to 365)" },
        "directory": { "kind": "businesses|people (default businesses)", "sort": "businesses: name|kind|owner|district|street|staff|revenue_today|revenue_yesterday|profit_yesterday|money|rating|footfall|state; people: name|age|class|income|job|home|household|mood|health|commute|favourite|friends|status", "desc": "bool", "search": "words to find in name, kind, owner, street, district, workplace, home", "district": "district name", "business_kind": "businesses: a kind key", "state": "businesses: building|shut|broke|no_staff|open|closed", "class": "people: low|middle|high|elite", "status": "people: working|at_home|asleep|out|travelling|ill|homeless|arriving|away", "offset": "default 0", "limit": "default 50, max 1000", "show": "bool: also open City Data's Directory page with these settings", "note": "the rows of City Data's Directory tab, sorted and filtered by the page's own code; missing values sort last" },
        "list": { "what": "businesses|buildings|citizens|lots|routes|plants|facilities", "sort": "column", "desc": "bool", "filter": "{column: substring | value | {min, max}}", "fields": "[columns] (id always kept)", "offset": "default 0", "limit": "default 50, max 1000" },
        "street_events": { "limit": "rows of history and news (default 10)", "fresh": "bool: weigh the warehouses for a rave now (else the last 22:00 roll's)", "returns": "race (stage, meet, finish, route_m, racers with place and speed, crowd, incident, arrests, fines), race_planned, race_crew, rave (place, door, guests, queue, queue_at_door, inside, peak, complaints, incident, pull), rave_planned, rave_spots (warehouses with youth, artists, art_scene, neighbours, culture, has_scene, night_chance, why), history, news, totals" },
        "race": { "at": "[x,y] optional: meet at the empty lot or surface car park nearest it", "returns": "race id and detail; errors if fewer than two locals with a fast car are free, or no lot pair 0.9-2.4 km apart" },
        "rave": { "building": "id optional: the warehouse", "at": "[x,y] optional: the old warehouse nearest it", "returns": "rave id and detail (with no field: the warehouse with the strongest scene)" },
        "roads": { "funding": "optional 0..1.5: road maintenance funding level (1 = what the network needs to stand still)", "age_days": "dev, optional: wear every street that many days at yesterday's traffic (the crews work each night unless crews: false; nothing is charged)", "near": "[x,y] optional: list the pieces nearest it", "limit": "pieces listed (default 10, worst first)", "returns": "pieces, lane_km, mean_wear, worn, crumbling, waiting_for_repair, mean_wear_by_land_value {cheap, middle, dear}, budget {funding, need_per_day, allowance_per_day, spent_yesterday, repaired_yesterday, repaired_land_value_yesterday, ledger_today/yesterday, repaired_total}, days, pieces_listed (wear, condition, label, speed_factor, crash_factor, land_value, repair_cost, queue_place, street)" },
        "taxes": { "set": "{line: rate, ...} where line is a key (residential_low|residential_mid|residential_high|grocery|bakery|gadget_shop|parking|nightlife_low|nightlife_mid|nightlife_high|farming|oil|refinery|manufacturing|logistics|fishing|tech) or a land use for its whole group; rate 0.2 or 20 (= 20%)", "category": "one line (with rate)", "import_duty": "share of import prices (0..1)", "reset": "bool: back to the standard rates", "returns": "every line: rate, default, pull, buildings, homes/jobs, today, yesterday, recent_per_day; by_land_use; duty" },
        "tolls": { "action": "list (default) | price | remove | inspect", "id": "booth id (or at: [x,y])", "price": "toll per car (lorries pay double)", "returns": "booths with cars and revenue today/yesterday/total, paid/free today, upkeep" },
        "export": { "path": "optional .json (see files); default exports/city-<unix>.json: report + map + history + every row of every list" },
        "build_highlight": { "category": "a Build section (power|services|transit|rail|water|attractions), a sub-section (safety, health and education, sanitation, parking, metro, buses...) or a kind (health, police, fire, school, metro_station, coal...): opens the Build window there (a kind also picks it, as a click on its tile) and highlights those buildings on the map with their service areas", "off": "bool: close the Build window (the highlight goes with it)", "note": "no fields: what is highlighted now and the summary (worked out the frame after a change)" },
        "window": { "name": "a tool window: lines|view (or land)|saves|help|dev, or build (the Build window; section roads|zoning|parks|districts|power|services|transit|rail|water|attractions), new_game, title (the title screen), history (the Game history window); the old report names city|economy|taxes|market|people|power|transit|emergencies|districts|tourism open their City Data tab or page", "open": "bool, omitted toggles", "sub": "with a City Data name: the sub-tab (people's old tab overview|concerns|social works too)" },
        "skyline": { "action": "summary (default) | limit | sites | start | finish | fund", "limit": "limit: the city's height limit in metres, null or 0 for none (also cmd height_limit {limit})", "vertiport_clearance": "limit: bool, keep the approach round the player's vertiports clear (towers there held to 120 m)", "height": "start: metres (else the developer decides: over the record when demand and money allow)", "design": "start: deco_spire|bundled|twist|needle", "at": "start: [x,y] build as near here as a site allows", "force": "start (dev, default true): skip the money, demand, population and downtown tests (height limits and site rules still hold); sites: list every site that passes the limits", "rich": "start: give the developers $2B first", "amount": "fund (dev): dollars to every tower developer (default 1e9)", "limit_rows": "rows of the ranking and sites (default 15)", "returns": "summary: limit_m, vertiport_clearance, classes_m, record_m, tallest_planned_m, population_cap_m, demand, classes (standing/rising), towers (id, name, height_m, league, design, developer, lots, pos, under_construction, progress, rank, homes, jobs), ranking (the tallest standing), records, news, developers (name, capital), totals; inspect {building} has a tower block (height_m, rank, rank_label, height_limit_m; for a league tower league, design, developer, lots, pending_lots, site_m2, floors, started_day, topped_out_day, record_bid, progress, days_left)" },
        "city_data": { "tab": "city|economy|market|people|services|directory (omitted with open: the page it was on)", "sub": "the tab's sub-tab: city overview|land|districts|skyline|tourism; economy overview|cash_flow|ledger|taxes; market goods|businesses|trade; people population|inequality|concerns|social|favourites; services power|transit|emergencies; directory businesses|people (omitted: the tab's last one)", "open": "bool; omitted with a tab opens; no fields reports the view and the list of tabs", "scroll": "points from the page's top to scroll to (for screenshots of long pages)", "note": "the full-screen City Data view (J in the game, Esc closes); status.city_data says what it shows" },
        "concerns": { "days": "daily counts and happiness over the last n days (default 30)", "concern": "optional one of housing|food|jobs|safety|trash|noise|commute|parking|power|services|nightlife|displacement|other", "quotes": "recent quotes per concern (default 5)", "note": "what people complain about, ranked by people this week with the trend against last week, where it comes from, and city happiness (daily, hourly for 48 h)" },
        "social": { "days": "daily history to include (default 7)", "talks": "latest conversations to list (default 10)", "group": "optional group id: that group in full, with its members", "note": "people talking: today's and yesterday's conversations, friendships, feuds, fights, meet-ups, jobs and flats through friends, couples moving in; pairs by bond city-wide; every crew and club with momentum, reputation, base and works (gigs, matches, readings, murals, gardens, businesses, petitions, protests); petitions to the player; murals, gardens, gigs; news. inspect {citizen} has a social block (ties with bond and how they met, groups, recent conversations)" }
    });
    if let (Some(c), Some(r)) = (v["commands"].as_object_mut(), review.as_object()) {
        c.extend(r.clone());
    }
    // [mapgen] (out of the literal above: it is at the json! recursion limit)
    // [land]
    v["commands"]["estate"] = json!({ "action": "summary (default: counters, floors histogram, the billionaire gate, elites with their convoys, NIMBY zones, forms, owners) | pressure ({at: [x,y]} for one point, else every busy cell) | spawn_elite (dev: a billionaire now, needs quiet land) | expand {building} (dev: branch or merge now) | buyout {building} (dev) | drive {citizen, to} (dev)" });
    v["commands"]["ownership"] = json!({ "building": "id: what to highlight when it is selected: the owner and every property with its outline (a rich person's other homes and businesses too)" });
    v["commands"]["set_weather"] = json!({ "cloud": "0..1 (rain or snow above 0.6, by climate)", "wind": "0.05..1", "temp_c": "-50..55", "note": "dev: sets the sky now; it drifts again hour by hour" });
    v["commands"]["ships"] = json!({ "returns": "every ship and boat: freighters, ferries, marina boats and (fishing) trawlers with name, port, at, heading_deg, state (in_port|outbound|fishing|hauling|returning|unloading), at_sea, nets_out, fish_aboard, catch_today; count, trawlers_at_sea" });
    v["commands"]["generate_city"] = json!({ "preset": "grid_plaza|coastal_river|old_town|twin_centres (default grid_plaza)", "seed": "u64 (default 1): the city and its landscape", "landscape": "optional landscape preset to lay it on instead of the city preset's own (river_valley|islands|coast|delta|plains|flat)", "water": "0..1", "relief": "0..1", "branching": "0..1", "climate": "temperate|desert|tropical|arctic|volcanic (default: the city preset's, desert)", "note": "throws the city away: a new game with a whole generated city (tensor-field streets, zoning, highway interchanges, a coal plant); returns the landscape summary and city {arterial, collector, local, zones, lots, interchanges, way_in, centres}" });
    // [districts] [tourism]
    v["commands"]["district"] = json!({ "action": "list (default)|create|paint|erase|set|remove|at|select|layer", "id": "district id (or district: id or name)", "at": "[x,y] brush centre", "radius": "m, default 40", "polygon": "[[x,y],...]", "rect": "[[x0,y0],[x1,y1]]", "residential": "kowloon|suburban|eco|none", "commercial": "neon_strip|tech_campus|night_market|tourist|none", "industry": "farming|oil|refinery|manufacturing|logistics|fishing|tech|none", "historic": "bool", "height_limit": "metres (null or 0 lifts it): nothing new rises over it in the district", "rename": "new name", "on": "bool (layer)" });
    v["commands"]["tourism"] = json!({ "action": "refresh (re-read attractions now)", "limit": "tourists listed, default 20", "returns": "attractiveness, demand, ways in, today's and yesterday's arrivals, spending and tourist tax, hotels, attractions, tourists" });
    // [society]
    // [subway]
    v["commands"]["subway"] = json!({ "action": "status (default)|station|line|preview|headway|remove_line|inspect_line|inspect_station", "x": "f (station)", "y": "f (station)", "stations": "[id or [x,y], ...] (line, preview)", "points": "[[x,y], ...] (line: places stations where none is near)", "loop": "bool (line, preview): back to the first station", "line": "id", "minutes": "headway", "headway": "minutes (line)", "station": "id or [x,y]" });
    // [metro-ux]
    v["commands"]["metro_line"] = json!({ "stations": "[id or [x,y], ...]: clicked in order, as the player does", "click": "id or [x,y]: one more", "loop": "bool: click the first station again", "undo": "bool or n: take back stops", "finish": "bool: lay the line", "cancel": "bool: drop the draft first", "headway": "minutes (finish)", "tool": "bool (default true): pick the metro line tool, which shows the Metro layer" });
    // The player's selection and flags (added here: the big literal above
    // is at the json! macro's recursion limit).
    v["commands"]["selected"] = json!({ "": "full detail of what the player has selected: a car with why (reason, text, by), jam (chain to the head of the queue, head_reason, cycle), trip (elapsed_min, expected_min, anger), lanes, driver, cargo; a citizen (and driving_car); a building; a parked car; the bus line picked (bus_line)" });
    v["commands"]["parking_price"] = json!({ "building": "id of a parking lot the city runs", "price": "the fee, 0..50", "returns": "building, price (as set, clamped)" });
    v["commands"]["flags"] = json!({ "since": "flag id: only newer ones", "clear": "bool: empty the list after reading" });
    v["commands"]["flag"] = json!({ "note": "text: flag the current selection, as F8 does in game" });
    v["commands"]["walk_to"] = json!({ "citizen": "id", "to": "[x,y]", "from": "[x,y] (default: where they are)", "note": "dev: sends the citizen out on foot by the walk graph (footbridges included)" });
    v["commands"]["intersection"] = json!({ "at": "[x,y] near a junction or crosswalk (default: the editor's)", "signal": "auto|signals|none", "greens": "[seconds per phase, ...] (phases as listed) or \"default\"", "left_arrow": "auto|always|never", "arrow_seconds": "3..30", "left_turns": "bool: allow (true) or forbid every left", "no_left": "[approach index or [x,y], ...]: approaches with the left forbidden (the rest allowed)", "approach": "index or [x,y] (with lanes)", "lanes": "[\"L\", \"S+R\", ...] leftmost first, or \"reset\"", "crossing": "index or [x,y]", "upgrade": "none|button|footbridge (with crossing; a footbridge replaces the crosswalk under it)", "keep_crosswalk": "bool (with crossing): keep the crosswalk at grade under its footbridge (default false: the bridge replaces it)", "open": "bool: show the Intersection editor modal", "close": "bool", "select_approach": "index (open)", "select_crossing": "walk crossing index (open)", "returns": "intersection: at, signalized, rule, approaches (lanes, offered, left_forbidden), phases (green_s, running), crossings (upgrade, signalised, button_light, crosswalk_kept, crosswalk_removed); done; editor_open" });
    v["commands"]["society"] = json!({ "action": "report (default)|evict|offend|trash", "building": "id (evict: its households go on the street; offend: the target; trash: one building's bins)", "citizen": "id (offend)", "kind": "theft|burglary|assault|murder|vandalism (offend, with building)", "kg": "trash to add, default 100" });
    // Planning: several projects, and painting parcels.
    let planning = json!({
        "tool": { "name": "inspect|road|zone|edit|bulldoze|power|transit|rail", "preset": "Path|Rail|Lane|Street|Avenue|One-way|Ramp|Highway|Tunnel|Roundabout (Tunnel switches the view underground, the others back to ground)", "elevation": "road tool level for the next point, m (-20..24)", "depth": "underground|ground|sky: the view layer", "draw": "[[x,y,level?],...]: the road being drawn, for its preview", "cursor": "[x,y]: where the pointer stands for the road preview (until the mouse is used)", "diameter": "roundabout 24..240 m", "ring_lanes": "roundabout 1|2|3", "ring_at": "[x,y]: show the roundabout preview there as if hovered (until the mouse is used)", "kind": "sub-mode of the active tool", "mode": "zone tool: polygon|brush|marquee", "radius": "zone brush radius m", "unzone": "bool", "industry": "zone tool sub-zone for industrial land: auto|farming|oil|...", "land_use": "(or none: the brush unzones)", "lanes_fwd": "", "lanes_bwd": "", "curved": "", "show_routes": "", "route": "id" },
        "project": { "action": "list|new|switch|rename|delete|park|start|implement|undo|redo|discard", "id": "project id (default: the active one)", "name": "for new/rename, or to pick a project by name", "note": "several named projects; the active one is edited, the rest previewed dim; implement builds one at a time, the later one wins where two change the same gesture or parcel (warnings in the reply)" },
        "paint": { "land_use": "residential|...|park, or none to unzone", "at": "[x,y] brush centre", "radius": "m (default the tool's)", "industry": "industrial only: auto|farming|oil|refinery|manufacturing|logistics|fishing|tech|mining|smelting|factory_farming, the sub-zone set on implement", "rect": "[[x0,y0],[x1,y1]] marquee", "polygon": "[[x,y],...]", "implement": "bool", "note": "zones pre-cut parcels: brush = parcels the circle touches, rect/polygon = parcels whose centre is inside; parcels that refuse the use are listed with why; one call is one undo step" }
    });
    if let (Some(c), Some(r)) = (v["commands"].as_object_mut(), planning.as_object()) {
        c.extend(r.clone());
    }
    v
}

/// Every map layer and whether it is on, the one showing (`active`, null
/// for none: they are exclusive) and the "show other projects" switch.
fn overlays_json(o: &Overlays) -> Value {
    let mut m = serde_json::Map::new();
    for l in crate::map_layers::MAP_LAYERS {
        m.insert(l.key.into(), json!(o.get(l.key).unwrap_or(false)));
    }
    m.insert("land".into(), json!(o.land.map(|l| l.key())));
    m.insert("active".into(), json!(crate::map_layers::active(o).map(|l| l.key())));
    m.insert("projects".into(), json!(o.projects));
    Value::Object(m)
}
