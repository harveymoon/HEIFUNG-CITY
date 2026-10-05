# Remote control API

The game runs a small HTTP JSON API on localhost so scripts, tests and AI assistants can drive
and inspect it without synthetic keystrokes or clicks. `tools/mcp/nd_mcp.py` wraps it as an MCP
server (see [`tools/mcp/README.md`](../tools/mcp/README.md)). The code is `crates/nd_app/src/remote.rs`.

## Transport

- `POST http://127.0.0.1:4817/cmd` with a JSON body `{"cmd": "<name>", ...fields}`.
- `GET /status` and `GET /help` as shortcuts for `{"cmd":"status"}` and `{"cmd":"help"}`.
- Replies are JSON: `{"ok": true, ...}` with HTTP 200, or `{"ok": false, "error": "..."}` with HTTP 400
  (unknown command, bad or missing field, the sim refusing, e.g. "not enough money"). 404 for other
  paths; 504 if the game did not answer within 5 s.
- Environment: `ND_REMOTE_PORT` picks the port (default 4817); `ND_REMOTE=0` disables the server.
  If the port is taken the game logs a warning and runs without the API.

### How it runs

A background thread accepts requests and queues each with a reply channel. The Bevy system
`remote_commands` (in `SimSet::Input`, before the player's tools) drains the queue every frame,
runs the commands on the main thread with full access to the sim, tools and camera, and sends
the replies back. So:

- **One frame of latency** per request (more while the window is minimised or a frame is slow).
- Commands within a frame run in arrival order; each sees the effects of the previous one.
- Effects that the renderer or the camera system apply (a camera move, a new road's meshes) show
  up on screen at the end of that frame or the next.

## Ids and coordinates

- Citizens, buildings, lots, cars, trains and bus routes are slotmap keys, sent as **`"idx:version"`
  strings**, e.g. `"42:1"`. They come from `find`, `status`, `inspect` and `place`, and go straight
  back into `inspect`, `select`, `camera {follow}`, `bus {route}`. A stale id (the thing is gone and
  the slot reused) fails with "no such ..." rather than pointing at something else. Ids survive
  save and load of the same city.
- Plan gestures (roads and zones as drawn) have plain integer ids (`gesture_id`).
- Positions are **`[x, y]` in sim metres** on the ground plane (the renderer's x and z). The starter
  avenues run along x = 0 and y = 0; the map spans about ±1000 m; the rail trunk is at y = -480.
- Times: a tick is one sim minute; `day` is 1-based.

## Commands

Every example is the POST body; the reply always carries `"ok"`.

### Overview and time

| Command | Fields | Reply |
| --- | --- | --- |
| `help` | | every command and its fields |
| `status` | | clock, paused, speed, effective_speed, population, buildings, under_construction, lots, homes/jobs, treasury, ledger_yesterday, power, weather, land, market, demand, traffic, transit, rail, render (fps, ticks_per_frame, triangles), camera, tool, selection, project, overlays, lighting, message, tool_status, event_seq, favourites |
| `pause` | `paused` bool (omit to toggle) | `paused` |
| `speed` | `index` 0..4 (1×, 4×, 16×, 64×, 256×) or `value` (sim minutes per real second, snapped) | `speed_index`, `speed`, `label` |
| `step` | `ticks` (default 1, max 100000) | `clock`, `took_ms` |

`step` runs the ticks immediately, on the main thread, even while paused (the same `World::tick`
the frame loop uses), and stalls the frame for as long as it takes: a young city runs thousands of
ticks a second, a big one far fewer, so keep each request well inside the 5 s reply timeout. A big
step also floods the event ring (see `events`).

```json
{"cmd": "pause", "paused": true}
{"cmd": "speed", "index": 3}
{"cmd": "step", "ticks": 200}
```

### Saves

| Command | Fields | Reply |
| --- | --- | --- |
| `seed_demo` | | lays out and implements the demo district (works on any map) |
| `save` | `path` optional | without a path: `saves/city.ron` plus a dated slot, like the Save button; with one: just that file. Reply `path` (absolute) |
| `load` | `path` | as the Load button: tools and selection reset, the city comes up paused |
| `list_saves` | | `saves`: path, modified_unix, bytes, newest first |

Relative paths are relative to the game's working directory (the repo root under `cargo run`).

### Planning: roads, zones, the project

Like the tools, new gestures go into the pending **project** and change nothing until it is
implemented. Pass `"implement": true` to implement straight away, or send `project` later.

| Command | Fields |
| --- | --- |
| `plan` | none: every gesture in the effective plan (id, kind, points, lanes, land_use), `pending_gesture_ids`, project steps |
| `add_road` | `points` `[[x,y],...]` (2+), `kind` street (default) / path / rail, `lanes_fwd`, `lanes_bwd` (0..4, default 1/1, streets only), `curved`, `snap` (default true: ends snap to the network like the Road tool), `implement` |
| `add_zone` | `polygon` `[[x,y],...]` (3+), `land_use` residential / commercial / industrial / entertainment / civic / park, `implement` |
| `bulldoze` | `gesture_id`, `implement` |
| `project` | `action`: start / implement / undo / redo / discard |

```json
{"cmd": "add_road", "points": [[300, -200], [300, 200]], "lanes_fwd": 2, "lanes_bwd": 2}
{"cmd": "add_zone", "polygon": [[310, -60], [400, -60], [400, 60], [310, 60]], "land_use": "residential"}
{"cmd": "project", "action": "implement"}
```

Reply: `gesture_id`, `implemented`, `project` (`steps`, `redo`), `plan_version`.

### Placing buildings

`place {kind, x, y}` (or `pos: [x, y]`):

- `station`, `freight_yard`: rail pieces, placed anywhere (`World::place_rail_facility`); the reply
  message says whether track and street are joined.
- `depot`, `parking`, `coal`, `solar`, `wind`, `nuclear`: on the lot under the point, or the nearest
  vacant lot within 80 m (`build_depot`, `build_parking`, `build_power`).

Reply: `building` id, `message`, `row`, `treasury`; or the sim's reason as `error`.

```json
{"cmd": "place", "kind": "solar", "x": -210, "y": -210}
```

### Buses

`bus {action, ...}`:

| action | fields |
| --- | --- |
| `new_route` | (none) |
| `add_stop` | `x`, `y`, `route` optional (default: the line being drawn, or a new one) |
| `move_stop` | `route`, `index`, `x`, `y` |
| `remove_stop` | `route`, `index` (the last stop removes the line) |
| `set_buses` | `route`, `n` |
| `remove_route` | `route` |
| `list` | |

```json
{"cmd": "bus", "action": "add_stop", "x": 0, "y": 136}
```

### Finding and inspecting

`find {what, near?, radius?, filter?, limit?}`, with `what` one of citizens, buildings, lots, cars,
trains, routes, gestures. With `near` the rows are nearest first and carry `distance`; `radius`
defaults to 100 m then. Filters (substring match on strings, case-insensitive):

- citizens: `activity` ("commuting", "at home", ...), `name`, `class` (low / middle / high), `visible`, `spotlight`
- buildings: `land_use`, `tier`, `business_kind` ("grocery", "parts fab", ...), `name`,
  `under_construction`, `plant`, `rail`, `depot`, `derelict`
- lots: `land_use`, `vacant`, `park`, `min_area`
- cars: `kind` (car, bus, truck); trains: `kind` (passenger, freight)

Reply: `total` (matches), `returned`, `rows`.

```json
{"cmd": "find", "what": "buildings", "near": [0, 0], "radius": 300, "filter": {"land_use": "commercial"}, "limit": 5}
```

`inspect` with one of `citizen`, `building`, `lot`, `car`, `train`, `route` (ids) or `gesture` (int);
with none, the current selection. The detail matches what the inspector and the person pane show:

- citizen: name, personality, activity (label, destination, car/bus/train ids), home/work, mood,
  satiety, class, income, rent, savings, groceries, car/bike, thought, memories (newest first)
- building: land use, tier, style, floors, built day, construction (progress, days left,
  deliveries, materials), land value, rent, wage, condition, homes/jobs and the people,
  footfall, power (powered share, demand), parking, plant, depot, rail link, business (cash,
  takings, needs and sells with stock and prices), park
- lot: land use, polygon, area, land value and tier, zone, building, vacant/landlocked, footfall

### Selection and the player's UI

| Command | Fields |
| --- | --- |
| `select` | `citizen` / `building` / `car` id, or `at: [x, y]` (picks like a click), or nothing to clear |
| `favourite` | `citizen`, `on` (omit to toggle) |
| `tool` | `name` (inspect, road, zone, edit, bulldoze, power, transit, rail), `preset` (Path, Rail, Lane, Street, Avenue, One-way, Ramp, Highway), `kind` (the active tool's sub-mode: road kind, plant, station / freight_yard, route / depot, land use), `land_use`, `lanes_fwd`, `lanes_bwd`, `curved`, `show_routes`, `route`, `selected_gesture` |
| `overlay` | `land_value`, `paths`, `zoning` (bools) |
| `lighting` | `follow_clock`, `hour` (0..24; setting an hour unpins lighting from the clock) |

### Camera

`camera` takes any of:

- `look_at: [x, y]` (orbit target), `distance` (12..2500), `yaw`, `pitch` (radians; pitch 0.08..1.5);
- `x`, `z` (target coordinates; `y` is accepted for `z`);
- `follow`: a citizen or car id, or `{"citizen"|"car"|"building": id}`; the target tracks it every
  frame until the next `camera` command;
- `mode`: orbit / walk (in walk mode `look_at`/`x`/`z` place the eye, yaw/pitch turn the head).

Reply: the pose (`mode`, `target`, `distance`, `yaw`, `pitch`, `eye`), and `following`.

```json
{"cmd": "camera", "look_at": [0, 0], "distance": 400, "pitch": 0.9}
```

### Screenshot

`screenshot {path?}`: queues a capture of the window (`Screenshot::primary_window()` with
`save_to_disk`) and replies at once with the absolute `path` (default
`<working dir>/screenshots/shot-<unix seconds>.png`). **The file is written a frame or two later**,
when the GPU copy lands: poll for it (the MCP tool waits up to 5 s). It captures the whole window,
UI panel included, at the window's size (a full-HD window makes a 2–5 MB PNG). PNG only. A
minimised window renders nothing and produces no file.

### Events

`events {since?, limit?}`: a ring buffer of the last 2000 events, each with a running `seq` and the
`tick`. Kinds: `BuildingBuilt`, `BuildingChanged`, `BuildingRemoved`, `Expropriated`,
`BusinessClosed`, `CitizenArrived`, `CitizenLeft`, `NewThought` (with the thought text), `NewDay`,
`RoadsChanged`, `LotsChanged`, `PlanChanged`, plus `Message` and `ToolStatus` whenever the status
lines change. Poll with `since` = the last `seq` you saw; `dropped: true` means events fell out of the
ring in between. `thoughts {limit?}`: the newest memories across all citizens, and current thoughts.

## Caveats

- **Localhost only**, no authentication: anything on the machine can drive the game. Do not
  forward the port. Set `ND_REMOTE=0` to turn it off.
- One frame of latency per request; the HTTP thread gives up after 5 s (HTTP 504), e.g. while the
  window is minimised and frames stall.
- Remote edits go through the same `World` calls as the tools, so they behave identically,
  including undo within a project, costs and refusals.
- `load` replaces the city: all ids from before are invalid.
