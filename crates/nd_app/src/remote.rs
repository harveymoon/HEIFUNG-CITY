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

// The plumbing: the HTTP thread, the bundle commands run against, the
// dispatcher and its help.
mod ctx;
mod dispatch;
mod events;
mod help;
mod server;
/// Request checks (Host, Origin, content type, body size) and the folders
/// file commands may use.
mod guard;
mod view;

// Commands, by area.
mod bus;
mod camera;
mod city;
mod dev;
mod files;
mod flags;
mod incidents;
mod inspect;
mod look;
mod place;
mod planning;
mod review;
mod status;
mod time;
mod tool;
mod window;

// Feature commands, report sections and inspector blocks.
/// [committee] `committee`, `committee_meeting`.
mod committee;
/// [concerns] complaints by category and city happiness.
mod concerns;
/// [megacorp] Mega-corps and universities.
mod corp;
/// [rail-crossing] Level crossings.
mod crossings;
/// [directory] `directory`: City Data's Directory rows, sorted and filtered.
mod directory;
/// [districts] [tourism] `district` and `tourism`.
mod districts;
/// [economy] business pressure: what the city's businesses want
mod economy;
/// [ferry-routes] A dock's routes and `inspect {ship}`.
mod ferry;
/// [camera] `follow`, `zoom_to`, `gift`; delivery robots' detail.
mod follow;
mod grounds;
mod history;
/// [identity] `city_name` and `game_history`.
mod identity;
mod junction;
mod land;
mod list;
mod map;
/// [events] street races, raves, road condition
mod nights;
/// [places] taste, favourite places, venue ratings.
mod places;
/// [land] Owners, prices, merged plots, pressure and the super-elite.
mod property;
mod report;
/// [road-inspect] A street piece in full for `inspect {road}`.
mod road;
/// [waterfront] Waterfront lots, public beaches and private jetties.
mod shore;
/// [supertall] the skyscraper league
mod skyline;
/// [social] Friends, feuds, groups.
mod social;
/// [society] crime, homelessness and trash.
mod society;
/// [subway] The `subway` command and report section.
mod subway;
mod taxes;
/// [taxis] Taxi companies and cargo hubs in the report and inspector.
mod taxis;
/// [toll-inspect] A toll booth in full for `inspect {toll}`.
mod tolls;
mod trains;
/// [fire-scene] Emergency units: inspect, select, list.
mod units;

use bevy::prelude::*;

use crate::sim::SimSet;
use dispatch::remote_commands;
use events::{log_events, record_history, EventLog};
use history::History;
use server::{inbox_waiting, serve};
use window::WindowRequests;

pub use inspect::{selected_detail, selection_json_brief, tick_time_json};

const DEFAULT_PORT: u16 = 4817;

pub struct RemotePlugin;

impl Plugin for RemotePlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("ND_REMOTE").is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("off")) {
            info!("remote API disabled (ND_REMOTE=0)");
            return;
        }
        let port = std::env::var("ND_REMOTE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
        let (queue, inbox) = server::queue();
        match tiny_http::Server::http(("127.0.0.1", port)) {
            Ok(server) => {
                info!("remote API listening on http://127.0.0.1:{port}  (POST /cmd, GET /status)");
                std::thread::Builder::new().name("nd-remote-http".into()).spawn(move || serve(server, queue, port)).expect("spawn remote http thread");
            }
            Err(e) => {
                warn!("remote API could not bind 127.0.0.1:{port}: {e}; set ND_REMOTE_PORT or ND_REMOTE=0");
                return;
            }
        }
        app.insert_resource(inbox)
            .init_resource::<EventLog>()
            .init_resource::<History>()
            .init_resource::<WindowRequests>()
            .add_systems(Update, remote_commands.run_if(inbox_waiting).in_set(SimSet::Input).before(crate::tools::tool_input).before(crate::city_reset::on_city_replaced))
            .add_systems(Update, (log_events, record_history).in_set(SimSet::Sync))
            // Before the panel draws, so a request shows the same frame.
            .add_systems(bevy_egui::EguiPrimaryContextPass, window::apply_window_requests.before(crate::ui::side_panel));
    }
}
