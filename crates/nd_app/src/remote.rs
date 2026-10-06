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

mod history;
mod land;
mod list;
mod map;
mod report;
mod view;
mod window;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
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
use crate::sim::{Sim, SimSet, SPEEDS};
use crate::tools::{EffectivePlan, Selection, Tool, ToolState, TransitMode, ROAD_PRESETS};
use crate::ui::{load_now, save_now, Saves, UiFlags};
use history::History;
use view::*;
use window::WindowRequests;

const DEFAULT_PORT: u16 = 4817;
/// How long the HTTP thread waits for the main thread to answer. A frame
/// normally takes milliseconds; a long `step` or a stalled window can take
/// longer, and the caller gets a 504 rather than hanging forever.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
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
                std::thread::Builder::new().name("nd-remote-http".into()).spawn(move || serve(server, tx)).expect("spawn remote http thread");
            }
            Err(e) => {
                warn!("remote API could not bind 127.0.0.1:{port}: {e}; set ND_REMOTE_PORT or ND_REMOTE=0");
                return;
            }
        }
        app.insert_resource(Inbox(Mutex::new(rx)))
            .init_resource::<EventLog>()
            .init_resource::<Follow>()
            .init_resource::<History>()
            .init_resource::<WindowRequests>()
            .add_systems(Update, remote_commands.in_set(SimSet::Input).before(crate::tools::tool_input))
            .add_systems(Update, (log_events, follow_target, record_history).in_set(SimSet::Sync))
            .add_systems(bevy_egui::EguiPrimaryContextPass, window::apply_window_requests);
    }
}

/// One parked request: the parsed command and where the reply goes.
struct Pending {
    cmd: Value,
    reply: Sender<Reply>,
}

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

/// `camera {follow: id}` keeps the orbit target on a moving thing until the
/// next camera command.
#[derive(Resource, Default)]
struct Follow(Option<Selection>);

// ── HTTP thread ─────────────────────────────────────────────────────────────

fn serve(server: tiny_http::Server, tx: Sender<Pending>) {
    for mut req in server.incoming_requests() {
        // Owned copies: reading the body borrows the request mutably.
        let method = req.method().as_str().to_string();
        let url = req.url().split('?').next().unwrap_or("").to_string();
        let cmd = match (method.as_str(), url.as_str()) {
            ("GET", "/status") | ("GET", "/status/") => Ok(json!({ "cmd": "status" })),
            ("GET", "/help") => Ok(json!({ "cmd": "help" })),
            ("POST", "/cmd") | ("POST", "/cmd/") | ("POST", "/") => {
                let mut body = String::new();
                match req.as_reader().read_to_string(&mut body) {
                    Ok(_) => serde_json::from_str::<Value>(&body).map_err(|e| (400, format!("body is not JSON: {e}"))),
                    Err(e) => Err((400, format!("could not read body: {e}"))),
                }
            }
            _ => Err((404, "use POST /cmd with a JSON body {\"cmd\": ...}, or GET /status".to_string())),
        };
        let reply = match cmd {
            Err((status, error)) => Reply { status, body: json!({ "ok": false, "error": error }) },
            Ok(cmd) => {
                let (reply_tx, reply_rx) = mpsc::channel();
                if tx.send(Pending { cmd, reply: reply_tx }).is_err() {
                    // The app is shutting down; the receiver is gone.
                    return;
                }
                match reply_rx.recv_timeout(REPLY_TIMEOUT) {
                    Ok(r) => r,
                    Err(_) => Reply { status: 504, body: json!({ "ok": false, "error": "the game did not answer in time (frame stalled or window closing)" }) },
                }
            }
        };
        let text = reply.body.to_string();
        let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).expect("static header");
        let _ = req.respond(tiny_http::Response::from_string(text).with_status_code(reply.status).with_header(header));
    }
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
    follow: ResMut<'w, Follow>,
    log: ResMut<'w, EventLog>,
    history: ResMut<'w, History>,
    windows: ResMut<'w, WindowRequests>,
    cams: Query<'w, 's, &'static mut OrbitCamera>,
    stats: Res<'w, RenderStats>,
    assets: Res<'w, GameAssets>,
    diagnostics: Res<'w, DiagnosticsStore>,
    commands: Commands<'w, 's>,
}

fn remote_commands(inbox: Res<Inbox>, mut ctx: Ctx) {
    // Take the batch out first: the lock must not be held while commands run.
    let batch: Vec<Pending> = match inbox.0.lock() {
        Ok(rx) => rx.try_iter().collect(),
        Err(_) => return,
    };
    for p in batch {
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
    history.observe(&sim.world);
}

/// Keep the orbit target on the followed citizen or car; drop the follow
/// when they are gone.
fn follow_target(sim: Res<Sim>, mut follow: ResMut<Follow>, mut cams: Query<&mut OrbitCamera>) {
    let Some(sel) = follow.0 else { return };
    let w = &sim.world;
    let pos = match sel {
        Selection::Citizen(id) => w.citizens.get(id).map(|c| match &c.activity {
            nd_sim::Activity::Driving { car, .. } => w.traffic.cars.get(*car).map_or(c.pos, |car| car.pos),
            nd_sim::Activity::Riding { bus } => w.traffic.cars.get(*bus).map_or(c.pos, |car| car.pos),
            _ => c.pos,
        }),
        Selection::Car(id) => w.traffic.cars.get(id).map(|c| c.pos),
        Selection::Building(id) => w.buildings.get(id).and_then(|b| w.lots.get(b.lot)).map(|l| l.center),
        Selection::None => None,
    };
    match pos {
        Some(p) => {
            for mut cam in &mut cams {
                cam.target = Vec3::new(p.x, 0.0, p.y);
            }
        }
        None => follow.0 = None,
    }
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
                // The nearest step on a log scale: 10 picks 16×, 2 picks 1×.
                let v = v.max(0.01).ln();
                sim.speed_index = SPEEDS.iter().enumerate().min_by(|a, b| ((a.1).1.ln() - v).abs().total_cmp(&((b.1).1.ln() - v).abs())).map_or(1, |(i, _)| i);
            } else {
                return Err("speed needs {index: 0..4} or {value: minutes per second}".into());
            }
            Ok(json!({ "speed_index": sim.speed_index, "speed": sim.speed(), "label": SPEEDS[sim.speed_index].0 }))
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
            ctx.sim.seed_demo_district();
            ctx.tools.status = "Demo district laid out around the centre".into();
            Ok(json!({ "buildings": ctx.sim.world.buildings.len(), "lots": ctx.sim.world.lots.len() }))
        }
        "save" => save(cmd, ctx),
        "load" => {
            let path = PathBuf::from(req_str(cmd, "path")?);
            let Ctx { sim, tools, selection, eplan, flags, saves, follow, .. } = &mut *ctx;
            load_now(&path, sim, tools, selection, eplan, flags);
            saves.mark_dirty();
            follow.0 = None;
            if flags.message.starts_with("load failed") {
                return Err(flags.message.clone());
            }
            ctx.history.reset(&ctx.sim.world, "city loaded");
            Ok(json!({ "message": ctx.flags.message, "clock": clock(&ctx.sim), "paused": ctx.sim.paused }))
        }
        "new_game" => {
            let params = land::params_from(cmd)?;
            ctx.sim.new_game(params);
            *ctx.tools = ToolState::default();
            *ctx.selection = Selection::None;
            ctx.eplan.invalidate();
            ctx.follow.0 = None;
            ctx.tools.status = format!("New city: {} landscape", ctx.sim.world.landscape.params.preset.label());
            ctx.history.reset(&ctx.sim.world, "new game");
            Ok(land::summary(&ctx.sim.world))
        }
        "terrain_at" => Ok(land::terrain_at(&ctx.sim.world, req_point(cmd)?)),
        "landscape" => Ok(land::summary(&ctx.sim.world)),
        "grant" => {
            // Dev: money for trying things out.
            let amount = opt_f32(cmd, "amount")?.unwrap_or(100_000.0);
            ctx.sim.world.economy.treasury += amount;
            Ok(json!({ "treasury": round(ctx.sim.world.economy.treasury) }))
        }
        "list_saves" => Ok(list_saves()),
        "plan" => Ok(plan(ctx)),
        "add_road" => add_road(cmd, ctx),
        "add_zone" => add_zone(cmd, ctx),
        "bulldoze" => {
            let id = GestureId(opt_u64(cmd, "gesture_id")?.ok_or("missing gesture_id")?);
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
        "project" => {
            let action = req_str(cmd, "action")?;
            let w = &mut ctx.sim.world;
            let result = match action {
                "start" => {
                    w.start_project();
                    true
                }
                "implement" => {
                    let had = w.has_project();
                    w.implement();
                    ctx.tools.points.clear();
                    ctx.tools.status = "Project implemented".into();
                    had
                }
                "undo" => w.undo(),
                "redo" => w.redo(),
                "discard" => {
                    let had = w.has_project();
                    w.discard_project();
                    ctx.tools.points.clear();
                    had
                }
                _ => return Err(format!("unknown project action {action:?}; one of start, implement, undo, redo, discard")),
            };
            let mut v = project_state(ctx);
            v["did"] = json!(result);
            Ok(v)
        }
        "place" => place(cmd, ctx),
        "bus" => bus(cmd, ctx),
        "find" => find(cmd, ctx),
        "inspect" => inspect(cmd, ctx),
        "select" => select(cmd, ctx),
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
        "overlay" => {
            let o = &mut ctx.overlays;
            if let Some(b) = opt_bool(cmd, "land_value")? {
                o.land_value = b;
            }
            if let Some(b) = opt_bool(cmd, "paths")? {
                o.paths = b;
            }
            if let Some(b) = opt_bool(cmd, "zoning")? {
                o.zoning = b;
            }
            Ok(json!({ "land_value": o.land_value, "paths": o.paths, "zoning": o.zoning }))
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
            Ok(json!({ "follow_clock": l.follow_clock, "hour": l.hour }))
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
        "window" if req_str(cmd, "name").is_ok_and(|n| n.trim().eq_ignore_ascii_case("new_game") || n.trim().eq_ignore_ascii_case("new game")) => {
            let open = opt_bool(cmd, "open")?.unwrap_or(true);
            ctx.windows.new_game = Some(open);
            Ok(json!({ "window": "new game", "open": open }))
        }
        "window" => {
            let info = window::info_of(req_str(cmd, "name")?)?;
            let (open, list) = ctx.windows.request(info, opt_bool(cmd, "open")?);
            Ok(json!({ "window": info.title().to_ascii_lowercase(), "open": open, "windows_open": list }))
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
        "name": tool_name(t.tool), "road_kind": road_kind_name(t.kind), "lanes_fwd": t.lanes_fwd, "lanes_bwd": t.lanes_bwd, "curved": t.curved,
        "land_use": t.land_use.label(), "power_kind": plant_name(t.power_kind), "rail_kind": rail_name(t.rail_kind),
        "transit_mode": transit_mode_name(t.transit_mode), "route": opt_key(t.route_sel), "show_routes": t.show_routes,
        "pending_points": t.points.len(), "selected_gesture": t.selected_gesture.map(|g| g.0), "status": t.status,
    })
}

fn project_state(ctx: &Ctx) -> Value {
    let w = &ctx.sim.world;
    json!({ "project": w.project.as_ref().map(|p| json!({ "steps": p.steps.len(), "redo": p.redo.len() })), "gestures": w.effective_plan().gestures.len(), "plan_version": w.plan_version })
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
    let mut v = json!({
        "clock": clock(sim), "paused": sim.paused, "speed_index": sim.speed_index, "speed": sim.speed(), "speed_label": SPEEDS[sim.speed_index].0,
        "effective_speed": round(sim.effective_speed), "ticks_last_frame": sim.ticks_last_frame,
        "population": w.citizens.len(), "buildings": w.buildings.len() - sites, "under_construction": sites,
        "lots": w.lots.len(), "lots_vacant": w.lots.values().filter(|l| l.building.is_none() && l.usable()).count(), "lots_unusable": w.lots.values().filter(|l| l.landlocked).count(),
        "homes": { "used": e.homes_total - e.homes_free, "total": e.homes_total }, "jobs": { "used": e.jobs_total - e.jobs_free, "total": e.jobs_total },
        "waiting_immigrants": e.waiting_immigrants, "unhoused": e.unhoused, "unemployed": e.unemployed,
        "treasury": round(e.treasury), "tax_today": round(e.tax_today), "ledger_yesterday": ledger,
        "power": { "supply_mw": round(p.supply_mw), "imported_mw": round(p.imported_mw), "demand_mw": round(p.demand_mw), "served": round(p.served), "plants": p.plants, "unpowered_buildings": p.unpowered_buildings, "brownout_hours_today": p.brownout_hours_today, "upkeep_today": round(p.upkeep_today), "import_cost_today": round(p.import_cost_today) },
        "weather": { "label": w.weather.label(), "cloud": round(w.weather.cloud), "wind": round(w.weather.wind) },
        "land": { "avg_land_value": round(e.avg_land_value), "gini": round(e.gini), "displaced_total": e.displaced_total, "emigrated_total": e.emigrated_total, "priced_out_total": e.priced_out_total, "tier_counts": e.tier_counts, "class_counts": e.class_counts, "avg_rent": e.avg_rent.map(round) },
        "market": { "businesses": e.market.businesses, "deals_today": e.market.deals_today, "imports_today": round(e.market.imports_today), "exports_today": round(e.market.exports_today), "meals_sold_today": e.market.meals_sold_today, "hungry": e.market.hungry, "closed_total": e.market.closed_total },
        "demand": nd_sim::LandUse::ALL.iter().map(|&lu| (lu.label().to_string(), json!(round(e.demand_for(lu))))).collect::<serde_json::Map<_, _>>(),
        "traffic": { "cars": w.traffic.cars.len(), "lanes": w.traffic.net.lanes.len(), "signals": w.traffic.net.intersections.iter().filter(|i| i.signalized).count(), "parking_used": e.parking_used, "parking_capacity": e.parking_capacity, "walk_trips_today": e.walk_trips_today },
        "transit": { "routes": w.transit.routes.len(), "buses": t.buses, "depots": t.depots, "depot_capacity": t.depot_capacity, "waiting": t.waiting, "riding": t.riding, "riders_today": t.riders_today, "fares_today": round(t.fares_today), "upkeep_today": round(t.upkeep_today) },
        "rail": { "trains": r.trains, "stations": r.stations, "yards": r.yards, "waiting": r.waiting, "riding": r.riding, "riders_today": r.riders_today, "fares_today": round(r.fares_today), "next_passenger_in": r.next_passenger_in, "freight_in_today": round(r.freight_in_today), "freight_waiting": round(r.freight_waiting), "upkeep_today": round(r.upkeep_today) },
        "render": { "fps": round(fps as f32), "ticks_per_frame": sim.ticks_last_frame, "triangles": ctx.stats.street_triangles + ctx.stats.building_triangles, "building_meshes": ctx.stats.building_entities },
        "camera": camera_pose(ctx), "tool": tool_state(&ctx.tools), "selection": selection(w, *ctx.selection),
        "project": project_state(ctx)["project"], "overlays": { "land_value": ctx.overlays.land_value, "paths": ctx.overlays.paths, "zoning": ctx.overlays.zoning },
        "lighting": { "follow_clock": ctx.lighting.follow_clock, "hour": round(ctx.lighting.hour) },
        "message": ctx.flags.message, "tool_status": ctx.tools.status, "event_seq": ctx.log.seq,
        "favourites": ctx.tools.favorites.iter().map(|&f| key_str(f)).collect::<Vec<_>>(),
    });
    // Added after the literal: one `json!` this size hits the macro recursion limit.
    v["windows_open"] = json!(ctx.windows.open_names());
    v["alerts_top"] = report::alerts_json(&report::alerts(w), 5);
    v["review_hint"] = json!("for a whole-city review call report (then map, history, list)");
    v
}

// ── saves ───────────────────────────────────────────────────────────────────

fn save(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    match opt_str(cmd, "path")? {
        Some(path) => {
            ctx.sim.save(path)?;
            ctx.saves.mark_dirty();
            ctx.flags.message = format!("saved {path}");
            Ok(json!({ "path": absolute(path) }))
        }
        None => {
            let Ctx { sim, saves, flags, .. } = ctx;
            save_now(sim, saves, flags);
            if flags.message.starts_with("save failed") {
                return Err(flags.message.clone());
            }
            Ok(json!({ "path": absolute(crate::ui::LATEST), "message": flags.message }))
        }
    }
}

fn list_saves() -> Value {
    let mut rows: Vec<(u64, Value)> = std::fs::read_dir("saves")
        .map(|dir| {
            dir.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "ron"))
                .map(|e| {
                    let modified = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                    let size = e.metadata().map_or(0, |m| m.len());
                    (modified, json!({ "path": e.path().to_string_lossy().replace('\\', "/"), "modified_unix": modified, "bytes": size }))
                })
                .collect()
        })
        .unwrap_or_default();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    json!({ "saves": rows.into_iter().map(|(_, v)| v).collect::<Vec<_>>(), "latest": crate::ui::LATEST })
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
    v
}

fn add_road(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let mut pts = req_points(cmd, "points")?;
    if pts.len() < 2 {
        return Err("a road needs at least two points".into());
    }
    let kind = opt_str(cmd, "kind")?.map(road_kind_of).transpose()?.unwrap_or(RoadKind::Street);
    let (lf, lb) = if kind == RoadKind::Street { (opt_u64(cmd, "lanes_fwd")?.unwrap_or(1) as u8, opt_u64(cmd, "lanes_bwd")?.unwrap_or(1) as u8) } else { (0, 0) };
    if kind == RoadKind::Street && lf + lb == 0 {
        return Err("a street needs at least one lane".into());
    }
    if lf > 4 || lb > 4 {
        return Err("lanes are 0..4 per direction".into());
    }
    // Snap the ends to the network like the Road tool does, so the road
    // joins existing roads instead of stopping a metre short of them.
    if opt_bool(cmd, "snap")?.unwrap_or(true) {
        for p in &mut pts {
            *p = ToolState::road_snap(&ctx.sim, *p, kind).0;
        }
    }
    let curved = opt_bool(cmd, "curved")?.unwrap_or(false) && pts.len() >= 3;
    let id = ctx.sim.world.add_gesture(Gesture::Road { points: pts, lanes_fwd: lf, lanes_bwd: lb, curved, kind });
    let implemented = opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        ctx.sim.world.implement();
    }
    ctx.tools.status = format!("{} added by remote{}", match kind { RoadKind::Path => "path", RoadKind::Rail => "track", RoadKind::Street => "road" }, if implemented { " and implemented" } else { " (pending)" });
    let mut v = project_state(ctx);
    v["gesture_id"] = json!(id.0);
    v["implemented"] = json!(implemented);
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
    v["implemented"] = json!(implemented);
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
        "station" | "freight_yard" | "yard" => {
            let fac = if kind == "station" { RailFacility::Station } else { RailFacility::FreightYard };
            let id = w.place_rail_facility(fac, p)?;
            let link = w.buildings.get(id).and_then(|b| b.rail.as_ref()).map(|l| (l.rail_connected, l.road_connected));
            let note = match link {
                Some((true, true)) => "placed and connected",
                Some((true, false)) => "placed on the track; join a road to its street stub",
                Some((false, true)) => "placed by the street; lay track onto its rail stub",
                Some((false, false)) => "placed; needs track to its rail stub and a road to its street stub",
                None => "placed",
            };
            (id, note.to_string())
        }
        "depot" => {
            let lot = lot_near(w, p).ok_or("no lot there: click on or near a vacant lot by a street")?;
            (w.build_depot(lot)?, "bus depot built".into())
        }
        "parking" => {
            let lot = lot_near(w, p).ok_or("no lot there")?;
            (w.build_parking(lot).ok_or("cannot build parking there: the lot is taken, landlocked or too small")?, "parking built".into())
        }
        other if land::dock_of(other).is_some() => {
            let dock = land::dock_of(other).unwrap();
            let id = w.place_port(dock, p)?;
            let port = w.ports.port_of(id).cloned();
            let note = match port {
                Some(p) if p.works() => format!("{} built with its pier; ships call from the next sailing", dock.label()),
                Some(p) if !p.road_connected => format!("{} built with its pier; join its street stub to the city's roads", dock.label()),
                _ => format!("{} built, but no ship can reach it from off the map", dock.label()),
            };
            (id, note)
        }
        other => {
            let plant = plant_of(other).ok_or_else(|| format!("unknown kind {other:?}; one of station, freight_yard, depot, parking, coal, solar, wind, nuclear"))?;
            let lot = lot_near(w, p).ok_or("no lot there: aim at a vacant lot")?;
            let id = w.build_power(lot, plant)?;
            (id, format!("{} built for {:.0}", plant.label(), plant.build_cost()))
        }
    };
    *ctx.selection = Selection::Building(id);
    ctx.tools.status = note.clone();
    let b = ctx.sim.world.buildings.get(id).map(|b| building_row(&ctx.sim.world, id, b)).unwrap_or(Value::Null);
    Ok(json!({ "building": key_str(id), "message": note, "row": b, "treasury": round(ctx.sim.world.economy.treasury) }))
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
        _ => Err(format!("unknown bus action {action:?}; one of new_route, add_stop, move_stop, remove_stop, set_buses, remove_route, list")),
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
        "routes" | "route" | "lines" => {
            for (id, r) in w.transit.routes.iter() {
                let d = near.map_or(0.0, |n| r.stops.iter().map(|s| s.pos.distance(n)).fold(f32::INFINITY, f32::min));
                if d <= radius {
                    rows.push((d, route_row(id, r)));
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
        other => return Err(format!("unknown what {other:?}; one of citizens, buildings, lots, cars, trains, routes, gestures")),
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

fn inspect(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    if let Some(v) = field(cmd, "citizen") {
        let id = key_from(v)?;
        let c = w.citizens.get(id).ok_or("no such citizen")?;
        return Ok(json!({ "citizen": citizen_detail(w, id, c, ctx.tools.favorites.contains(&id)) }));
    }
    if let Some(v) = field(cmd, "building") {
        let id = key_from(v)?;
        let b = w.buildings.get(id).ok_or("no such building")?;
        let style = b.port.map(|k| k.label()).or(b.plant.map(|k| k.label())).unwrap_or(ctx.assets.rules.pick_style(b.land_use, b.tier, &mut nd_sim::rng_for(b.seed, 11)).label());
        return Ok(json!({ "building": building_detail(w, id, b, style) }));
    }
    if let Some(v) = field(cmd, "lot") {
        let id = key_from(v)?;
        let l = w.lots.get(id).ok_or("no such lot")?;
        return Ok(json!({ "lot": lot_detail(w, id, l) }));
    }
    if let Some(v) = field(cmd, "car") {
        let id = key_from(v)?;
        let c = w.traffic.cars.get(id).ok_or("no such car")?;
        return Ok(json!({ "car": car_detail(w, id, c) }));
    }
    if let Some(v) = field(cmd, "train") {
        let id = key_from(v)?;
        let t = w.rail.trains.get(id).ok_or("no such train")?;
        return Ok(json!({ "train": train_row(w, id, t) }));
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
    // No id: whatever is selected.
    match *ctx.selection {
        Selection::Citizen(id) => inspect(&json!({ "citizen": key_str(id) }), ctx),
        Selection::Building(id) => inspect(&json!({ "building": key_str(id) }), ctx),
        Selection::Car(id) => inspect(&json!({ "car": key_str(id) }), ctx),
        Selection::None => Err("inspect needs one of citizen, building, lot, car, train, route, gesture (an id), or a selection".into()),
    }
}

fn select(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let sel = if let Some(v) = field(cmd, "citizen") {
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
    } else if let Some(p) = opt_vec2(cmd, "at")? {
        // Pick like a click: use the same radii the Inspect tool does.
        pick_at(w, p)
    } else {
        Selection::None
    };
    *ctx.selection = sel;
    Ok(json!({ "selection": selection(w, sel) }))
}

/// The Inspect tool's pick, in miniature: a visible person, then a vehicle
/// (a driven car selects its driver), then the building whose lot holds the
/// point.
fn pick_at(w: &nd_sim::World, p: SVec2) -> Selection {
    let person = w.citizens.iter().filter(|(_, c)| c.is_visible()).map(|(id, c)| (c.pos.distance(p), id)).filter(|(d, _)| *d < 7.0).min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, id)) = person {
        return Selection::Citizen(id);
    }
    let car = w.traffic.cars.iter().map(|(id, c)| (c.pos.distance(p), id, c)).filter(|(d, _, c)| *d < if c.bus.is_some() { 8.0 } else { 5.0 }).min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, id, car)) = car {
        return if car.bus.is_none() && w.citizens.contains_key(car.citizen) { Selection::Citizen(car.citizen) } else { Selection::Car(id) };
    }
    w.lots.values().find(|l| l.building.is_some() && point_in_polygon(p, &l.polygon)).and_then(|l| l.building).map_or(Selection::None, Selection::Building)
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
    if let Some(preset) = opt_str(cmd, "preset")? {
        let (_, f, b, k) = ROAD_PRESETS.iter().find(|(l, ..)| l.eq_ignore_ascii_case(preset)).ok_or_else(|| format!("unknown preset {preset:?}; one of {}", ROAD_PRESETS.iter().map(|p| p.0).collect::<Vec<_>>().join(", ")))?;
        t.kind = *k;
        if *k == RoadKind::Street {
            t.lanes_fwd = *f;
            t.lanes_bwd = *b;
        }
    }
    if let Some(kind) = opt_str(cmd, "kind")? {
        // `kind` means whatever the active tool chooses between.
        match t.tool {
            Tool::Road | Tool::Edit => t.kind = road_kind_of(kind)?,
            Tool::Power => t.power_kind = plant_of(kind).ok_or_else(|| format!("unknown plant {kind:?}; one of coal, solar, wind, nuclear"))?,
            Tool::Rail => {
                t.rail_kind = match kind.to_ascii_lowercase().as_str() {
                    "station" => RailFacility::Station,
                    "freight_yard" | "yard" => RailFacility::FreightYard,
                    _ => return Err(format!("unknown rail kind {kind:?}; station or freight_yard")),
                }
            }
            Tool::Transit => {
                t.transit_mode = match kind.to_ascii_lowercase().as_str() {
                    "route" | "lines" => TransitMode::Route,
                    "depot" => TransitMode::Depot,
                    _ => return Err(format!("unknown transit mode {kind:?}; route or depot")),
                };
                t.route_sel = None;
            }
            Tool::Zone => t.land_use = land_use_of(kind)?,
            Tool::Inspect | Tool::Bulldoze => return Err("this tool has no kind".into()),
        }
    }
    if let Some(lu) = opt_str(cmd, "land_use")? {
        t.land_use = land_use_of(lu)?;
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
    Ok(json!({ "tool": tool_state(t) }))
}

fn camera(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    // Any explicit pose ends a follow; `follow` below may start a new one.
    ctx.follow.0 = None;
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
            _ => return Err("follow: one of citizen, car, building".into()),
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
            let p = match sel {
                Selection::Citizen(id) => w.citizens.get(id).map(|c| c.pos),
                Selection::Car(id) => w.traffic.cars.get(id).map(|c| c.pos),
                Selection::Building(id) => w.buildings.get(id).and_then(|b| w.lots.get(b.lot)).map(|l| l.center),
                Selection::None => None,
            };
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
    ctx.follow.0 = follow;
    Ok(json!({ "camera": camera_pose(ctx), "following": follow.map(|s| selection(&ctx.sim.world, s)) }))
}

// ── screenshot ──────────────────────────────────────────────────────────────

fn screenshot(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let path = match opt_str(cmd, "path")? {
        Some(p) => PathBuf::from(p),
        None => {
            let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            PathBuf::from("screenshots").join(format!("shot-{secs}.png"))
        }
    };
    let path = if path.is_absolute() { path } else { std::env::current_dir().map_err(|e| e.to_string())?.join(path) };
    // PNG only: Bevy's `jpeg` image feature is not enabled in this build,
    // and the failure would surface only as a log line, not as a reply.
    if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("png")) {
        return Err("screenshot path must end in .png".into());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
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
    let path = match opt_str(cmd, "path")? {
        Some(p) => PathBuf::from(p),
        None => {
            let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            PathBuf::from("exports").join(format!("city-{secs}.json"))
        }
    };
    let path = if path.is_absolute() { path } else { std::env::current_dir().map_err(|e| e.to_string())?.join(path) };
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
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(&path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(json!({ "path": path.to_string_lossy().replace('\\', "/"), "bytes": text.len(), "took_ms": ms(start) }))
}

// ── help ────────────────────────────────────────────────────────────────────

fn help() -> Value {
    let mut v = json!({
        "transport": "POST http://127.0.0.1:4817/cmd with JSON {\"cmd\": name, ...fields}; GET /status. Replies {ok: true, ...} or {ok: false, error} with HTTP 400.",
        "start_here": "to review the city: status -> report (all sections, ranked alerts) -> map {layer: hunger|land_value|...} for where -> history for trends -> list for tables -> inspect ids",
        "ids": "slotmap ids are \"idx:version\" strings (citizen, building, lot, car, train, route); gesture ids are plain integers. Positions are [x, y] sim metres.",
        "commands": {
            "help": {},
            "status": {},
            "pause": { "paused": "bool, omitted toggles" },
            "speed": { "index": "0..4", "value": "sim minutes per real second, snapped to the nearest step" },
            "step": { "ticks": "advance n sim minutes now, even while paused (default 1, max 100000)" },
            "seed_demo": {},
            "save": { "path": "optional; default writes saves/city.ron and a dated slot" },
            "load": { "path": "a .ron save" },
            "list_saves": {},
            "plan": {},
            "add_road": { "points": "[[x,y],...] (2+)", "lanes_fwd": "0..4 (streets)", "lanes_bwd": "0..4", "kind": "street|path|rail", "curved": "bool", "snap": "bool, default true", "implement": "bool, default false (pending in the project)" },
            "add_zone": { "polygon": "[[x,y],...] (3+)", "land_use": "residential|commercial|industrial|entertainment|civic|park", "implement": "bool" },
            "bulldoze": { "gesture_id": "int", "implement": "bool" },
            "project": { "action": "start|implement|undo|redo|discard" },
            "place": { "kind": "station|freight_yard|depot|parking|coal|solar|wind|nuclear|hydro|tidal|geothermal|shipping_port|ferry_terminal", "x": "f", "y": "f", "note": "hydro beside a river or stream with a current >= 0.8 m/s, tidal on the sea shore, geothermal over a hot spot; docks at the water's edge (they build a pier and a street stub)" },
            "new_game": { "preset": "river_valley|islands|coast|delta|plains|flat (default river_valley)", "seed": "u64", "water": "0..1", "relief": "0..1 height variation", "branching": "0..1", "islands": "0..8", "highway": "bool", "note": "throws the city away; omitted levers take the preset's" },
            "terrain_at": { "x": "f", "y": "f", "returns": "natural/ground/surface heights, slope, water (kind, current, width), nearest shore, hot spot, power sites and dock placement for the lot there" },
            "grant": { "amount": "money added to the treasury (dev), default 100000" },
            "landscape": { "returns": "preset and levers, rivers (width, current, hydro_ok), lakes, sea, hot spots, highway ends and through cars, bridges, docks, ships" },
            "bus": { "action": "new_route|add_stop|move_stop|remove_stop|set_buses|remove_route|list", "route": "id", "x": "f", "y": "f", "index": "stop index", "n": "buses" },
            "find": { "what": "citizens|buildings|lots|cars|trains|routes|gestures", "near": "[x,y]", "radius": "m (default 100 with near)", "filter": "object; citizens: activity,name,class,visible,spotlight; buildings: land_use,tier,business_kind,name,under_construction,plant,rail,depot,derelict; lots: land_use,vacant,park,min_area; cars/trains: kind", "limit": "default 50" },
            "inspect": { "citizen|building|lot|car|train|route": "id", "gesture": "int; with no id, the current selection" },
            "select": { "citizen|building|car": "id", "at": "[x,y] picks like a click; nothing clears" },
            "favourite": { "citizen": "id", "on": "bool, omitted toggles" },
            "tool": { "name": "inspect|road|zone|edit|bulldoze|power|transit|rail", "preset": "Path|Rail|Lane|Street|Avenue|One-way|Ramp|Highway", "kind": "sub-mode of the active tool", "land_use": "", "lanes_fwd": "", "lanes_bwd": "", "curved": "", "show_routes": "", "route": "id" },
            "camera": { "look_at": "[x,y]", "x": "", "z": "", "distance": "12..2500", "yaw": "rad", "pitch": "rad", "follow": "citizen/car id or {citizen|car|building: id}; any camera command ends a follow", "mode": "orbit|walk" },
            "overlay": { "land_value": "bool", "paths": "bool", "zoning": "bool" },
            "lighting": { "follow_clock": "bool", "hour": "0..24 (also unpins from the clock)" },
            "screenshot": { "path": "optional .png; default screenshots/shot-<unix>.png; written a frame or two later" },
            "events": { "since": "seq", "limit": "default 500" },
            "thoughts": { "limit": "default 50" }
        }
    });
    // The review commands, added separately: one `json!` this size hits the
    // macro recursion limit.
    let review = json!({
        "report": { "sections": "optional subset of overview, people, economy, land, services, alerts (default all): the one-call city review" },
        "map": { "cell_m": "cell size in metres, default 100 (grown to keep at most 400 cells)", "layer": "summary (table of cells) | population | land_value | jobs | hunger | vacancy | power | traffic | fertility | oil | contamination | industry (grid + ASCII heat map)" },
        "industry": { "lot": "lot id, or", "polygon": "[[x, y], ...] area, or", "pos": "[x, y] (query only)", "kind": "farming | oil | refinery | manufacturing | logistics | fishing | tech | none: sets the industry sub-zone bias (none hands the area back to the ground); omit to query the ground, contamination and industry weights of the lot" },
        "history": { "keys": "optional daily/hourly series names (default all)", "days": "last n days (default all kept, up to 365)" },
        "list": { "what": "businesses|buildings|citizens|lots|routes|plants|facilities", "sort": "column", "desc": "bool", "filter": "{column: substring | value | {min, max}}", "fields": "[columns] (id always kept)", "offset": "default 0", "limit": "default 50, max 1000" },
        "export": { "path": "optional; default exports/city-<unix>.json: report + map + history + every row of every list" },
        "window": { "name": "city|economy|market|people|power|transit|land|saves|help|dev", "open": "bool, omitted toggles" }
    });
    if let (Some(c), Some(r)) = (v["commands"].as_object_mut(), review.as_object()) {
        c.extend(r.clone());
    }
    v
}
