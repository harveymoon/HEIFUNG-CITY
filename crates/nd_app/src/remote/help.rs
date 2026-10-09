//! `help`, generated from the command registry (`dispatch::COMMANDS`):
//! every command with its fields, and every alias with its command.

use serde_json::{json, Map, Value};

use super::dispatch::COMMANDS;

const TRANSPORT: &str = "POST http://127.0.0.1:<port>/cmd (default port 4817) with JSON {\"cmd\": name, ...fields} and Content-Type: application/json (or the header X-ND-Remote: 1); GET /status, GET /help. Replies {ok: true, ...} or {ok: false, error} with HTTP 400. Requests from web pages (an Origin or Sec-Fetch-Site header from another site, a foreign Host) get 403; bodies over 1 MB 413; a 504 means the command was withdrawn unrun. See docs/REMOTE.md.";
const START_HERE: &str = "to review the city: status -> report (all sections, ranked alerts) -> map {layer: hunger|land_value|...} for where -> history for trends -> list for tables -> inspect ids";
const IDS: &str = "slotmap ids are \"idx:version\" strings (citizen, building, lot, car, train, route); gesture ids are plain integers. Positions are [x, y] sim metres.";
const FILES: &str = "save, load, export and screenshot paths must lie inside saves/, screenshots/ or exports/ under the game's working directory, or the OS temp folder; no '..'; extensions .ron, .json, .png";

pub(super) fn help() -> Value {
    let commands: Map<String, Value> = COMMANDS.iter().map(|c| (c.name.to_string(), Value::Object(c.help.iter().map(|(field, text)| (field.to_string(), json!(text))).collect()))).collect();
    let aliases: Map<String, Value> = COMMANDS.iter().flat_map(|c| c.aliases.iter().map(move |a| (a.to_string(), json!(c.name)))).collect();
    json!({ "transport": TRANSPORT, "start_here": START_HERE, "ids": IDS, "files": FILES, "commands": commands, "aliases": aliases })
}
