#!/usr/bin/env python3
"""MCP server for District 2040 (Neon District).

Wraps the game's localhost HTTP API (docs/REMOTE.md) as MCP tools, so an MCP
client such as Claude Code can drive and inspect a running game. Python 3
standard library only; speaks MCP over stdio (JSON-RPC 2.0, one message per
line).

    claude mcp add --scope user neon-district -- python C:\\CODE\\Neon_District\\tools\\mcp\\nd_mcp.py

The game must be running (it listens on 127.0.0.1:4817 unless ND_REMOTE_PORT
says otherwise). Every tool forwards one command and returns the game's JSON
reply as text; `screenshot` also waits for the PNG and attaches it as an image.
"""

import base64
import json
import os
import sys
import time
import urllib.error
import urllib.request

PORT = int(os.environ.get("ND_REMOTE_PORT", "4817"))
BASE = f"http://127.0.0.1:{PORT}"
SERVER_INFO = {"name": "neon-district", "version": "0.1.0"}
PROTOCOLS = ["2025-06-18", "2025-03-26", "2024-11-05"]
# Big screenshots blow up a conversation (and the Claude API refuses images
# over 5 MB); above this the image is not attached, only its path.
MAX_IMAGE_BYTES = int(os.environ.get("ND_MCP_MAX_IMAGE_BYTES", str(5 * 1024 * 1024)))


def call_game(cmd, timeout=15.0):
    """POST one command; return the decoded reply (ok or error)."""
    data = json.dumps(cmd).encode("utf-8")
    req = urllib.request.Request(BASE + "/cmd", data=data, headers={"Content-Type": "application/json", "X-ND-Remote": "1"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.loads(r.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        # 400/403/413/415/503/504 carry a JSON body {"ok": false, "error": ...}.
        try:
            return json.loads(e.read().decode("utf-8"))
        except Exception:
            return {"ok": False, "error": f"HTTP {e.code}"}
    except (urllib.error.URLError, ConnectionError, TimeoutError, OSError) as e:
        return {"ok": False, "error": f"game not reachable at {BASE} ({e}). Is District 2040 running? (ND_REMOTE_PORT={PORT})"}


# ── tool definitions ────────────────────────────────────────────────────────

POINT = {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "[x, y] in sim metres"}
POINTS = {"type": "array", "items": POINT}
ID = {"type": "string", "description": "slotmap id as \"idx:version\", e.g. \"12:1\""}


def obj(props, required=()):
    return {"type": "object", "properties": props, "required": list(required), "additionalProperties": True}


TOOLS = [
    {
        "name": "report",
        "description": "Use report first to review the whole city in one call: overview (population, homes, jobs, treasury, demand), people (class, activity, employment, housing, income/savings/mood distributions, rent burden, hunger, commute modes, top reasons from recent memories), economy (ledger, businesses by kind with staffing and stock vs target, resources supply vs demand and scarcity, most/least profitable businesses), land (lots and buildings by land use and tier, rents, land value), services (power plants, bus routes, rail facilities, parking, traffic) and alerts: a ranked list of concrete problems, each with a count, a location [x, y], example ids for inspect and a hint. Pass sections to get only some.",
        "inputSchema": obj({"sections": {"type": "array", "items": {"type": "string", "enum": ["overview", "people", "economy", "land", "services", "industry", "society", "alerts", "history"]}, "description": "default: all but history"}}),
    },
    {
        "name": "map",
        "description": "Use map for WHERE: the city cut into square cells (default 100 m, at most 400 cells). layer=summary gives a table of every non-empty cell (residents, homes, jobs, jobs filled, businesses, land value, hungry, vacant lots, unpowered, cars, dominant land use). A single layer (population, land_value, jobs, hunger, vacancy, power, traffic) gives its value grid and an ASCII heat map (' .:-=+*#%@', rows from the smallest y at the top), returned also as a separate text block. Cell (col,row) centre = [x0+(col+0.5)*cell_m, y0+(row+0.5)*cell_m].",
        "inputSchema": obj({
            "layer": {"type": "string", "enum": ["summary", "population", "people", "land_value", "jobs", "hunger", "vacancy", "power", "traffic", "fertility", "oil", "contamination", "industry", "crime", "homeless", "trash"]},
            "cell_m": {"type": "number", "minimum": 25, "description": "cell size in metres (default 100; grown if the grid would exceed 400 cells)"},
        }),
    },
    {
        "name": "history",
        "description": "Use history for TRENDS: daily series (population, households, treasury, net ledger, taxes, gini, land value, mood, hungry, unemployed, unhoused, businesses open/closed, buildings, sites, power, transit/rail riders, emigrated/displaced totals) for up to 365 days, and an hourly series of the last 48 hours (cars on the road, hungry, treasury, ...), with a first/last/change/min/max summary per key. Kept by the running game, not in saves: it restarts at app start and on load.",
        "inputSchema": obj({"keys": {"type": "array", "items": {"type": "string"}, "description": "series to return (default all)"}, "days": {"type": "integer", "minimum": 1, "description": "only the last n days"}}),
    },
    {
        "name": "list",
        "description": "Use list for TABLES: every row of one kind with consistent columns, sortable and filterable, e.g. the least staffed businesses or the citizens with the highest rent share. what=businesses|buildings|citizens|lots|routes|plants|facilities; sort=column, desc; filter={column: substring | value | {min, max}}; fields=[columns] (id always kept); offset, limit (default 50, max 1000). Row ids go into inspect/select/camera.",
        "inputSchema": obj({
            "what": {"type": "string", "enum": ["businesses", "buildings", "citizens", "lots", "routes", "plants", "facilities"]},
            "sort": {"type": "string"}, "desc": {"type": "boolean"}, "filter": {"type": "object"},
            "fields": {"type": "array", "items": {"type": "string"}}, "offset": {"type": "integer", "minimum": 0}, "limit": {"type": "integer", "minimum": 1, "maximum": 1000},
        }, ["what"]),
    },
    {
        "name": "export",
        "description": "Write the whole review (report, summary map, history, and every row of every list) to one JSON file for offline analysis; default <game dir>/exports/city-<unix>.json. Returns the path and size in bytes.",
        "inputSchema": obj({"path": {"type": "string", "description": "a .json path inside saves/, screenshots/ or exports/ (relative to the game dir) or the OS temp folder; no '..'"}}),
    },
    {
        "name": "window",
        "description": "Open, close or toggle one of the game's info windows (what the player sees): city, economy, market, people, power, transit, land, saves, help, dev. open omitted toggles. Returns the windows now open. Useful before a screenshot.",
        "inputSchema": obj({"name": {"type": "string", "enum": ["city", "economy", "taxes", "market", "people", "power", "transit", "emergencies", "land", "districts", "saves", "help", "dev", "new_game"]}, "open": {"type": "boolean"}}, ["name"]),
    },
    {
        "name": "selected",
        "description": "What the PLAYER has selected in the running game, in full: a car with why it stands (why.reason/text/by: red light, queued behind car X, yielding, lane change, at the kerb, unloading, crashed, takeover...), jam (the chain of cars back to the head of the queue, head_reason, cycle for gridlock), trip (elapsed_min vs expected_min, anger), lanes (lane ids, route ahead), driver profile, cargo; a citizen (and the car they are driving); a building; a parked car; the bus line picked. Use it when the player says 'this car' or 'what I selected'.",
        "inputSchema": obj({}),
    },
    {
        "name": "flags",
        "description": "The player's 'Flag for Claude' bookmarks (F8 in game, or the button in the inspector and person pane): each has a note, the time, the selection, a snapshot of its full detail when flagged (a stuck car may have moved on since) and its live detail now. since=flag id returns only newer ones; clear=true empties the list after reading. note=... with create=true flags the current selection yourself.",
        "inputSchema": obj({"since": {"type": "integer", "minimum": 0}, "clear": {"type": "boolean"}, "create": {"type": "boolean"}, "note": {"type": "string"}}),
    },
    {
        "name": "status",
        "description": "City overview: clock, pause/speed, population, buildings, treasury, power, transit, rail, fps, camera pose, active tool, selection, status messages.",
        "inputSchema": obj({}),
    },
    {
        "name": "control",
        "description": "Sim time control. action=pause|resume|toggle; action=speed with index (0..4 = 1x,4x,16x,64x,256x) or value (sim minutes per real second); action=step with ticks (sim minutes, runs immediately even while paused).",
        "inputSchema": obj({
            "action": {"type": "string", "enum": ["pause", "resume", "toggle", "speed", "step"]},
            "index": {"type": "integer", "minimum": 0, "maximum": 4},
            "value": {"type": "number"},
            "ticks": {"type": "integer", "minimum": 1, "maximum": 100000},
        }, ["action"]),
    },
    {
        "name": "build",
        "description": "Plan roads and zones. action=add_road (points, kind street|path|rail, lanes_fwd, lanes_bwd, curved), add_zone (polygon, land_use), bulldoze (gesture_id), project (project_action start|implement|undo|redo|discard), plan (list gestures). New gestures go into the pending project unless implement=true.",
        "inputSchema": obj({
            "action": {"type": "string", "enum": ["add_road", "add_zone", "bulldoze", "project", "plan"]},
            "points": POINTS,
            "kind": {"type": "string", "enum": ["street", "path", "rail"]},
            "lanes_fwd": {"type": "integer", "minimum": 0, "maximum": 4},
            "lanes_bwd": {"type": "integer", "minimum": 0, "maximum": 4},
            "curved": {"type": "boolean"},
            "snap": {"type": "boolean", "description": "snap road ends to the network (default true)"},
            "polygon": POINTS,
            "land_use": {"type": "string", "enum": ["residential", "commercial", "industrial", "entertainment", "civic", "park"]},
            "gesture_id": {"type": "integer"},
            "project_action": {"type": "string", "enum": ["start", "implement", "undo", "redo", "discard"]},
            "implement": {"type": "boolean"},
        }, ["action"]),
    },
    {
        "name": "place",
        "description": "Place a building piece at a point: rail pieces (placed anywhere), power plants, depots and parking (on the vacant lot under or nearest the point), docks at the water's edge, toll booths across a road, services and attractions on the lot under the point (with tier). Returns the building id or the game's reason it cannot build.",
        "inputSchema": obj({
            "kind": {"type": "string", "description": "station, freight_yard, central_station, metro_station, cargo_hub, depot, parking, coal, solar, wind, nuclear, hydro, tidal, geothermal, shipping_port, ferry_terminal, toll_booth, toll_plaza, vertiport, police, fire, hospital, clinic, school, jail, dispatch, monument, stadium, trash_depot, parking_enforcement (services take tier low|mid|high or a :tier suffix); see docs/REMOTE.md"},
            "tier": {"type": "string", "enum": ["low", "mid", "high"]},
            "x": {"type": "number"}, "y": {"type": "number"},
        }, ["kind", "x", "y"]),
    },
    {
        "name": "bus",
        "description": "Bus lines. action=new_route; add_stop (x, y, route optional: extends the line being drawn); move_stop (route, index, x, y); remove_stop (route, index); set_buses (route, n); remove_route (route); list.",
        "inputSchema": obj({
            "action": {"type": "string", "enum": ["new_route", "add_stop", "move_stop", "remove_stop", "set_buses", "remove_route", "list"]},
            "route": ID, "x": {"type": "number"}, "y": {"type": "number"},
            "index": {"type": "integer", "minimum": 0}, "n": {"type": "integer", "minimum": 0, "maximum": 20},
        }, ["action"]),
    },
    {
        "name": "find",
        "description": "Search the city. what=citizens|buildings|lots|cars|trains|routes|gestures, optional near [x,y] + radius (nearest first), filter object (citizens: activity,name,class,visible,spotlight; buildings: land_use,tier,business_kind,name,under_construction,plant,rail,depot,derelict; lots: land_use,vacant,park,min_area; cars/trains: kind), limit. Rows carry ids for inspect/select.",
        "inputSchema": obj({
            "what": {"type": "string", "enum": ["citizens", "buildings", "lots", "parcels", "cars", "trains", "drones", "routes", "gestures"]},
            "near": POINT, "radius": {"type": "number"}, "filter": {"type": "object"}, "limit": {"type": "integer", "minimum": 1},
        }, ["what"]),
    },
    {
        "name": "inspect",
        "description": "Full detail of one thing by id: citizen (personality, activity, home/work, mood, money, thought, memories), building (tier, floors, homes/jobs, business stock and prices, power, construction, rail link), lot, car, train, route, or gesture. With no id: the current selection.",
        "inputSchema": obj({"citizen": ID, "building": ID, "lot": ID, "car": ID, "train": ID, "drone": ID, "vehicle": {"type": "string", "description": "a private car's record id (owner, model, where it is parked)"}, "route": ID, "gesture": {"type": "integer"}}),
    },
    {
        "name": "select",
        "description": "Select a citizen, building or car (as clicking with the Inspect tool would), or pick at a point with at=[x,y]; no arguments clears the selection. favourite=true/false also stars/unstars a selected citizen.",
        "inputSchema": obj({"citizen": ID, "building": ID, "car": ID, "at": POINT, "favourite": {"type": "boolean"}}),
    },
    {
        "name": "camera",
        "description": "Move the camera. look_at [x,y] (+ distance), or absolute x/z/distance/yaw/pitch (radians), or follow a citizen/car id (kept centred until the next camera command), or mode orbit|walk. Returns the pose.",
        "inputSchema": obj({
            "look_at": POINT, "x": {"type": "number"}, "z": {"type": "number"},
            "distance": {"type": "number", "minimum": 12, "maximum": 2500}, "yaw": {"type": "number"}, "pitch": {"type": "number"},
            "follow": {"description": "citizen or car id, or {citizen|car|building: id}"},
            "mode": {"type": "string", "enum": ["orbit", "walk"]},
        }),
    },
    {
        "name": "view",
        "description": "Display settings: overlays (land_value, paths, zoning booleans), lighting (follow_clock, hour 0..24), and the active tool panel state (tool name, preset, kind, land_use, lanes, curved, show_routes).",
        "inputSchema": obj({
            "land_value": {"type": "boolean"}, "paths": {"type": "boolean"}, "zoning": {"type": "boolean"},
            "follow_clock": {"type": "boolean"}, "hour": {"type": "number", "minimum": 0, "maximum": 24},
            "tool": {"type": "string", "enum": ["inspect", "road", "zone", "edit", "bulldoze", "buildings", "power", "transit", "rail", "district"]},
            "preset": {"type": "string"}, "kind": {"type": "string"}, "land_use": {"type": "string"},
            "lanes_fwd": {"type": "integer"}, "lanes_bwd": {"type": "integer"}, "curved": {"type": "boolean"}, "show_routes": {"type": "boolean"},
        }),
    },
    {
        "name": "screenshot",
        "description": "Capture the game window to a PNG (default <game dir>/screenshots/shot-<unix>.png). Waits up to 5 s for the file and returns its path, plus the image itself unless include_image=false.",
        "inputSchema": obj({"path": {"type": "string", "description": "a .png path inside saves/, screenshots/ or exports/ (relative to the game dir) or the OS temp folder; no '..'"}, "include_image": {"type": "boolean"}}),
    },
    {
        "name": "events",
        "description": "Recent sim events (buildings built/removed, arrivals, departures, new thoughts, new day, status messages) with a running seq; pass since=<last seq> to poll for new ones. what=thoughts instead lists recent citizen memories across the city.",
        "inputSchema": obj({"since": {"type": "integer", "minimum": 0}, "limit": {"type": "integer", "minimum": 1}, "what": {"type": "string", "enum": ["events", "thoughts"]}}),
    },
    {
        "name": "saves",
        "description": "action=save (path optional: default writes saves/city.ron plus a dated slot), load (path; the city comes up paused; replacing a city with unsaved play time needs confirm=true), list.",
        "inputSchema": obj({"action": {"type": "string", "enum": ["save", "load", "list"]}, "path": {"type": "string", "description": "a .ron path inside saves/, screenshots/ or exports/ (relative to the game dir) or the OS temp folder; no '..'"}, "confirm": {"type": "boolean", "description": "load only: true to replace a running city whose play time no save holds (the game refuses without it and says what would be lost)"}}, ["action"]),
    },
    {
        "name": "seed_demo",
        "description": "Lay out the demo district (grid of streets, mixed zones, rail branch with station and freight yard, coal plant, bus depot and a line) and implement it at once.",
        "inputSchema": obj({}),
    },
    {
        "name": "raw",
        "description": "Send any command object to the game API as is, e.g. {\"cmd\": \"help\"}. See docs/REMOTE.md.",
        "inputSchema": obj({"command": {"type": "object", "description": "the JSON body, with a \"cmd\" field"}}, ["command"]),
    },
]


def pick(args, *names):
    """The named arguments that are present, as a dict."""
    return {k: args[k] for k in names if k in args and args[k] is not None}


def wait_for_file(path, timeout=5.0):
    """The screenshot lands a frame or two after the reply; wait until it is
    there and its size has stopped changing."""
    deadline = time.time() + timeout
    last = -1
    while time.time() < deadline:
        try:
            size = os.path.getsize(path)
            if size > 0 and size == last:
                return True
            last = size
        except OSError:
            pass
        time.sleep(0.1)
    return os.path.exists(path) and os.path.getsize(path) > 0


def run_tool(name, args):
    """Returns (reply dict, extra content blocks)."""
    if name == "status":
        return call_game({"cmd": "status"}), []
    if name == "selected":
        return call_game({"cmd": "selected"}), []
    if name == "flags":
        if args.get("create"):
            return call_game({"cmd": "flag", **pick(args, "note")}), []
        return call_game({"cmd": "flags", **pick(args, "since", "clear")}), []
    if name == "report":
        return call_game({"cmd": "report", **pick(args, "sections")}), []
    if name == "map":
        reply = call_game({"cmd": "map", **pick(args, "layer", "cell_m")})
        extra = []
        if reply.get("ok") and reply.get("ascii_text"):
            # The heat map as plain text, readable as is; the JSON keeps the numbers.
            extra.append({"type": "text", "text": reply.pop("ascii_text")})
        return reply, extra
    if name == "history":
        return call_game({"cmd": "history", **pick(args, "keys", "days")}), []
    if name == "list":
        return call_game({"cmd": "list", **pick(args, "what", "sort", "desc", "filter", "fields", "offset", "limit")}), []
    if name == "export":
        return call_game({"cmd": "export", **pick(args, "path")}, timeout=30.0), []
    if name == "window":
        return call_game({"cmd": "window", **pick(args, "name", "open")}), []
    if name == "control":
        a = args.get("action")
        if a in ("pause", "resume", "toggle"):
            cmd = {"cmd": "pause"}
            if a != "toggle":
                cmd["paused"] = a == "pause"
            return call_game(cmd), []
        if a == "speed":
            return call_game({"cmd": "speed", **pick(args, "index", "value")}), []
        if a == "step":
            n = int(args.get("ticks", 1))
            return call_game({"cmd": "step", "ticks": n}, timeout=max(15.0, n / 50.0)), []
        return {"ok": False, "error": f"unknown control action {a!r}"}, []
    if name == "build":
        a = args.get("action")
        if a == "add_road":
            return call_game({"cmd": "add_road", **pick(args, "points", "kind", "lanes_fwd", "lanes_bwd", "curved", "snap", "implement")}), []
        if a == "add_zone":
            return call_game({"cmd": "add_zone", **pick(args, "polygon", "land_use", "implement")}), []
        if a == "bulldoze":
            return call_game({"cmd": "bulldoze", **pick(args, "gesture_id", "implement")}), []
        if a == "project":
            return call_game({"cmd": "project", "action": args.get("project_action", "implement")}), []
        if a == "plan":
            return call_game({"cmd": "plan"}), []
        return {"ok": False, "error": f"unknown build action {a!r}"}, []
    if name == "place":
        return call_game({"cmd": "place", **pick(args, "kind", "tier", "x", "y")}), []
    if name == "bus":
        return call_game({"cmd": "bus", **pick(args, "action", "route", "x", "y", "index", "n")}), []
    if name == "find":
        return call_game({"cmd": "find", **pick(args, "what", "near", "radius", "filter", "limit")}), []
    if name == "inspect":
        return call_game({"cmd": "inspect", **pick(args, "citizen", "building", "lot", "car", "train", "drone", "vehicle", "route", "gesture")}), []
    if name == "select":
        reply = call_game({"cmd": "select", **pick(args, "citizen", "building", "car", "vehicle", "at")})
        if reply.get("ok") and "favourite" in args and reply.get("selection", {}) and (reply["selection"] or {}).get("kind") == "citizen":
            reply["favourite"] = call_game({"cmd": "favourite", "citizen": reply["selection"]["id"], "on": bool(args["favourite"])})
        return reply, []
    if name == "camera":
        return call_game({"cmd": "camera", **pick(args, "look_at", "x", "z", "distance", "yaw", "pitch", "follow", "mode")}), []
    if name == "view":
        out = {"ok": True}
        ov = pick(args, "land_value", "paths", "zoning")
        if ov:
            out["overlay"] = call_game({"cmd": "overlay", **ov})
        li = pick(args, "follow_clock", "hour")
        if li:
            out["lighting"] = call_game({"cmd": "lighting", **li})
        tl = pick(args, "preset", "kind", "land_use", "lanes_fwd", "lanes_bwd", "curved", "show_routes")
        if "tool" in args:
            tl["name"] = args["tool"]
        if tl:
            out["tool"] = call_game({"cmd": "tool", **tl})
        if len(out) == 1:
            out["status"] = {k: v for k, v in call_game({"cmd": "status"}).items() if k in ("overlays", "lighting", "tool")}
        out["ok"] = all(v.get("ok", True) for v in out.values() if isinstance(v, dict))
        return out, []
    if name == "screenshot":
        reply = call_game({"cmd": "screenshot", **pick(args, "path")})
        if not reply.get("ok"):
            return reply, []
        path = reply["path"]
        if not wait_for_file(path):
            reply["ok"] = False
            reply["error"] = f"screenshot requested but {path} did not appear within 5 s (window minimised?)"
            return reply, []
        size = os.path.getsize(path)
        reply["bytes"] = size
        extra = []
        if args.get("include_image", True):
            if size <= MAX_IMAGE_BYTES:
                with open(path, "rb") as f:
                    extra.append({"type": "image", "data": base64.b64encode(f.read()).decode("ascii"), "mimeType": "image/png"})
            else:
                reply["note"] = f"image not attached: {size} bytes > ND_MCP_MAX_IMAGE_BYTES"
        return reply, extra
    if name == "events":
        if args.get("what") == "thoughts":
            return call_game({"cmd": "thoughts", **pick(args, "limit")}), []
        return call_game({"cmd": "events", **pick(args, "since", "limit")}), []
    if name == "saves":
        a = args.get("action")
        if a == "save":
            return call_game({"cmd": "save", **pick(args, "path")}), []
        if a == "load":
            return call_game({"cmd": "load", **pick(args, "path", "confirm")}), []
        if a == "list":
            return call_game({"cmd": "list_saves"}), []
        return {"ok": False, "error": f"unknown saves action {a!r}"}, []
    if name == "seed_demo":
        return call_game({"cmd": "seed_demo"}), []
    if name == "raw":
        cmd = args.get("command")
        if isinstance(cmd, str):
            cmd = json.loads(cmd)
        return call_game(cmd), []
    return None, []


# ── JSON-RPC over stdio ─────────────────────────────────────────────────────


def send(msg):
    sys.stdout.write(json.dumps(msg, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def handle(msg):
    method = msg.get("method")
    mid = msg.get("id")
    params = msg.get("params") or {}
    if method == "initialize":
        asked = params.get("protocolVersion")
        version = asked if asked in PROTOCOLS else PROTOCOLS[0]
        return {
            "protocolVersion": version,
            "capabilities": {"tools": {"listChanged": False}},
            "serverInfo": SERVER_INFO,
            "instructions": "Drive the running District 2040 game. Start with `status`; to review how the city is doing call `report` first (ranked alerts with locations and ids), then `map` (where), `history` (trends), `list` (tables) and `inspect` (one thing); ids are \"idx:version\" strings from find/status; positions are [x, y] sim metres (the map spans about -1000..1000). Build with `build` (pending until implemented), then `control step` to let time pass.",
        }
    if method == "ping":
        return {}
    if method == "tools/list":
        return {"tools": TOOLS}
    if method == "tools/call":
        name = params.get("name")
        args = params.get("arguments") or {}
        try:
            reply, extra = run_tool(name, args)
        except Exception as e:  # a bad argument must not kill the server
            reply, extra = {"ok": False, "error": f"{type(e).__name__}: {e}"}, []
        if reply is None:
            raise RpcError(-32602, f"unknown tool {name!r}")
        content = [{"type": "text", "text": json.dumps(reply, indent=1)}] + extra
        return {"content": content, "isError": not reply.get("ok", False)}
    if mid is None:
        return None  # notifications (initialized, cancelled, ...) need no answer
    raise RpcError(-32601, f"method not found: {method}")


class RpcError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code
        self.message = message


def main():
    # Windows consoles default stdin/stdout to a code page; MCP is UTF-8.
    for stream in (sys.stdin, sys.stdout):
        try:
            stream.reconfigure(encoding="utf-8", newline="\n")
        except Exception:
            pass
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError as e:
            send({"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": f"parse error: {e}"}})
            continue
        batch = msg if isinstance(msg, list) else [msg]
        for m in batch:
            mid = m.get("id")
            try:
                result = handle(m)
                if mid is not None:
                    send({"jsonrpc": "2.0", "id": mid, "result": result if result is not None else {}})
            except RpcError as e:
                if mid is not None:
                    send({"jsonrpc": "2.0", "id": mid, "error": {"code": e.code, "message": e.message}})
            except Exception as e:
                if mid is not None:
                    send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32603, "message": f"{type(e).__name__}: {e}"}})


if __name__ == "__main__":
    main()
