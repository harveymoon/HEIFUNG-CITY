# Remote control API

The game runs a small HTTP JSON API on localhost so scripts, tests and AI assistants can drive
and inspect it without synthetic keystrokes or clicks. `tools/mcp/nd_mcp.py` wraps it as an MCP
server (see [`tools/mcp/README.md`](../tools/mcp/README.md)). The code is `crates/nd_app/src/remote.rs`.

## Transport

- `POST http://127.0.0.1:4817/cmd` with a JSON body `{"cmd": "<name>", ...fields}` and
  `Content-Type: application/json` (see [Who may call](#who-may-call) for the alternatives).
- `GET /status` and `GET /help` as shortcuts for `{"cmd":"status"}` and `{"cmd":"help"}`.
- Replies are JSON: `{"ok": true, ...}` with HTTP 200, or `{"ok": false, "error": "..."}` with:
  - 400: unknown command, bad or missing field, the sim refusing (e.g. "not enough money");
  - 403: the request came from a web page or named a foreign host (below);
  - 404: another path;
  - 413: a body over 1 MB;
  - 415: a POST without an accepted content type (below);
  - 503: 16 requests already in progress;
  - 504: the game did not take the command within 5 s (frame stalled, window closing). The command
    is **withdrawn and does not run**, so retrying is safe. A command the game had already started
    (a long `step`) is waited for, up to 5 minutes.
- Environment: `ND_REMOTE_PORT` picks the port (default 4817); `ND_REMOTE=0` disables the server.
  If the port is taken the game logs a warning and runs without the API.

```sh
curl http://127.0.0.1:4817/status
curl -d '{"cmd":"step","ticks":200}' http://127.0.0.1:4817/cmd
curl -H "Content-Type: application/json" -d '{"cmd":"pause"}' http://127.0.0.1:4817/cmd
```

### Who may call

The server binds 127.0.0.1 and has no authentication. Binding to localhost does not keep web pages
out: any page in a browser can send a "simple" POST to `http://127.0.0.1:4817` without a CORS
preflight, and DNS rebinding lets it read the replies too. Without checks, a page could save or
export over any file, or load and wipe the running city. So the server checks every request before
reading its body (`crates/nd_app/src/remote/guard.rs`):

- **`Host`**, when sent, must be `127.0.0.1:<port>` or `localhost:<port>`. A rebinding attack
  carries the attacker's host name and is refused (403).
- **`Origin`**: any `Origin` header is refused (403), `null` included, unless it is the server
  itself (`http://127.0.0.1:<port>` or `http://localhost:<port>`). Browsers send `Origin` on every
  cross-site POST.
- **`Sec-Fetch-Site`**, when sent, must be `same-origin` or `none`. A URL typed into the address
  bar sends `none`, so `GET /status` still opens in a browser.
- **POST content type**:
  - `application/json` is always accepted. A page cannot send it without a preflight, and the
    server never grants one.
  - Any request carrying the header `X-ND-Remote` (any value) is accepted. This header is also
    preflighted.
  - `application/x-www-form-urlencoded` (what plain `curl -d` sends), `text/plain` and no content
    type at all are accepted **only from clients that send no browser headers** (no `Origin`, no
    `Sec-Fetch-*`).
  - Anything else gets 415.

The MCP bridge sends `application/json` and `X-ND-Remote: 1`. `curl`, Python `urllib` and
`requests` work as shown above.

**Files.** `save`, `load`, `export` and `screenshot` take paths only inside these folders:

- `saves/`, `screenshots/` and `exports/` under the game's working directory (the repo root under
  `cargo run`). Any command may use any of the three, and their subfolders.
- The OS temp folder (`%TEMP%` on Windows, usually `/tmp` elsewhere).

A relative path is taken from the working directory (`saves/test.ron`). These are refused:

- paths with `..`;
- network and device paths (`\\server\share`, `//host/...`);
- other folders (`city.ron` at the top level, `C:/Users/...`);
- the wrong extension. `save` and `load` need `.ron`, `export` needs `.json` and `screenshot`
  needs `.png`.

Folders are resolved through symlinks and junctions before the check. Missing folders are
created.

**Limits.** Bodies are capped at 1 MB. Each request is served on its own thread, at most 16 at a
time, so a client that sends its body slowly holds up only itself.

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
| `status` | | clock, paused, speed, effective_speed, population, buildings, under_construction, lots, homes/jobs, treasury, ledger_yesterday, power, weather, land, market, demand, traffic, transit, rail, render (fps, ticks_per_frame, triangles), camera, tool, selection, project, overlays, lighting, message, tool_status, event_seq, favourites, `windows_open`, `window_expanded`, `build_window` {open, section}, `title_screen`, `alerts_top` (the first 5 of `report`'s alerts) |
| `pause` | `paused` bool (omit to toggle) | `paused` |
| `speed` | `index` 0..5 (1×, 4×, 16×, 64×, 256×, 1024×), or `value`: a multiple of real time for what moves, snapped to the nearest step on a log scale. At 1× people walk at their true pace; the clock runs 15× faster than that | `speed_index`, `speed`, `ticks_per_second`, `label` |
| `step` | `ticks` (default 1, max 100000) | `clock`, `took_ms` |

`step` runs the ticks immediately, on the main thread, even while paused (the same `World::tick`
the frame loop uses), and stalls the frame for as long as it takes: a young city runs thousands of
ticks a second, a big one far fewer. A step that has started is waited for (up to 5 minutes), but
the frame freezes meanwhile, so prefer several shorter steps. A big step also floods the event
ring (see `events`).

```json
{"cmd": "pause", "paused": true}
{"cmd": "speed", "index": 3}
{"cmd": "step", "ticks": 200}
```

### Saves

| Command | Fields | Reply |
| --- | --- | --- |
| `seed_demo` | | lays out and implements the demo district (works on any map) |
| `save` | `name` or `path`, both optional | `name`: a named game, as Saves, Save as: `saves/<name>.ron` (the name made file-safe: ASCII letters, digits and `-` kept, the rest `_`, at most 48 characters), a sidecar `saves/<name>.meta.json` (name as typed, day, population, treasury, buildings, landscape, saved_unix) and a top-down map picture `saves/<name>.png`, rendered offscreen and written about half a second later. Reply `path`, `name`, `thumbnail`, `meta` (absolute). Neither: `saves/city.ron` plus a dated slot, like Quick save (with a sidecar and `saves/city.png`). `path`: just that file |
| `load` | `path` (`.ron`, see the file rules) | as the Load button: tools and selection reset, the city comes up paused; closes the title screen |
| `list_saves` | | `saves`, newest first: `path`, `name` (as typed for named games; "Latest quick save", "Autosave", "Quick save, day N" otherwise), `kind` (named / latest / autosave / dated), `modified_unix`, `bytes`, `day`, `population`, `treasury`, `buildings`, `landscape` (null for saves without a sidecar), `thumbnail` (absolute PNG path, or null) |

The **title screen** comes up at startup (unless `ND_TITLE=0`) over a paused city: the saved
games as cards with their pictures, Continue, New game, Play this map. `load`, `new_game` and
`seed_demo` close it; `window {name: "title", open: false}` closes it and runs the city (as Play
this map), `open: true` brings it back. `status.title_screen` says whether it is up. Scripts that
screenshot the city should close it first.

Paths are limited to `saves/`, `screenshots/`, `exports/` and the temp folder (see
[Who may call](#who-may-call)). Relative ones are taken from the game's working directory (the
repo root under `cargo run`).

Every save is written to a temporary file and renamed over the target, so a crash mid-write
leaves the old file whole. The daily autosave (`saves/autosave.ron`) copies the world on the
main thread, then serialises and writes it on a worker thread.

### Landscape and new games

| Command | Fields | Reply |
| --- | --- | --- |
| `new_game` | `preset` river_valley (default) / islands / coast / delta / plains / flat, `seed`, `water` 0..1, `relief` 0..1 (height variation), `branching` 0..1, `islands` 0..8 (`highway` is ignored: every new game has the highway and the trunk line); omitted levers take the preset's | throws the city away and starts on the landscape; tools and selection reset. Reply as `landscape` |
| `landscape` | | preset and levers, `rivers` (kind river/stream, width, current_mps, hydro_ok, source, mouth, length), `lakes`, `sea`, `hot_spots`, `highway` (entries, exits, through_cars), `bridges` (count, metres, on the highway), `ports`, `ships`, `port_stats` |
| `terrain_at` | `x`, `y` | `natural_height`, `ground_height` (graded under roads and lots), `surface_height` (bridge decks included), `slope`, `water` (kind, current, width, flow), `nearest_shore`, `hot_spot`, `lot`, `power_sites` (hydro/tidal/geothermal ok or why not, for the lot there), `docks` (whether a shipping port or ferry terminal would go there, or why not), `survey` (every land survey layer's value 0..1 there) |
| `grant` | `amount` (default 100000) | dev: adds money to the treasury |

```json
{"cmd": "new_game", "preset": "delta", "seed": 12, "water": 0.7, "branching": 0.9}
{"cmd": "terrain_at", "x": -420, "y": 100}
```

`window {name: "new_game"}` opens the New Game window (preset list, levers, mini-map).

### Planning: roads, zones, parcels, projects

Like the tools, new gestures go into the active **project** and change nothing until it is
implemented. Pass `"implement": true` to implement straight away, or send `project` later.
Several named projects can be pending; the active one is edited, the others are previewed dim.
Parcels exist along every road before anything is zoned; zoning (a polygon, or `paint`) gives
them a land use.

| Command | Fields |
| --- | --- |
| `plan` | none: every gesture in the effective plan (id, kind, points, lanes, land_use), `pending_gesture_ids`, project steps |
| `add_road` | `points` `[[x,y],...]` or `[[x,y,level],...]` (2+; level metres above the ground, negative below), `elevation` (a level for every point without its own, or a list), `tunnel` (default level -8 m), `kind` street (default) / path / rail, `lanes_fwd`, `lanes_bwd` (0..4, default 1/1, streets only), `curved`, `snap` (default true: points snap to a road at about their level; an end with no level of its own snaps to any road and takes its level), `implement`. Reply adds `grade_warnings` (too steep, crossing at an awkward height). Refused (400) if it would meet the railway at a level neither at grade nor clear of it, e.g. a portal ramp part way down: the error says how far to lower or raise it |
| `add_roundabout` | `center` `[x,y]` (or `pos` / `x`,`y`; snaps onto a junction within 12 m or a street within 6 m), `radius` 12..120 m to the ring centreline or `diameter` 24..240 (default the tool's), `lanes` 1 / 2 / 3, `implement`. Reply adds `roundabout` {`center`, `radius`, `diameter`, `lanes`, `joins`, `cuts_roads` (gesture ids), `takes_buildings`}; refused where it would partly overlap another roundabout (nesting one wholly inside another, with a lane and pavement between, is fine) |
| `add_zone` | `polygon` `[[x,y],...]` (3+), `land_use` residential / commercial / industrial / entertainment / civic / park, `implement` |
| `bulldoze` | `gesture_id`, `implement` |
| `paint` | `land_use` (or `none` to unzone), then `at` `[x,y]` + `radius` (the brush), `rect` `[[x0,y0],[x1,y1]]` (the marquee) or `polygon`; `industry` (industrial only: auto / farming / oil / refinery / manufacturing / logistics / fishing / tech), `implement`. Reply: `painted`, `touched`, `refused` (`[{parcel, center, why}]`), `gesture_id` |
| `project` | `action`: list / new / switch / rename / delete / park / start / implement / undo / redo / discard; `id` (default the active project) or `name` to pick one; `name` for new and rename. Reply: `projects` (`id`, `name`, `active`, `steps`, `redo`, `conflicts` `[{gesture, project}]`, `shared_parcels`), `warnings` from the last implement |
| `find` `{what: "parcels"}` | per parcel: `index`, `center`, `area`, `land_use` (built), `planned`, `landlocked`, `street`, `rough`, `accepts`, `lot`; filter `land_use`, `zoned`, `landlocked` |

```json
{"cmd": "add_road", "points": [[300, -200], [300, 200]], "lanes_fwd": 2, "lanes_bwd": 2}
{"cmd": "add_zone", "polygon": [[310, -60], [400, -60], [400, 60], [310, 60]], "land_use": "residential"}
{"cmd": "project", "action": "implement"}
{"cmd": "project", "action": "new", "name": "Harbour"}
{"cmd": "paint", "land_use": "industrial", "industry": "logistics", "rect": [[300, 10], [420, 60]]}
{"cmd": "project", "action": "switch", "id": 1}
```

Reply: `gesture_id`, `implemented`, `project` (`id`, `name`, `steps`, `redo`), `projects`, `plan_version`.

Road works never take the city's own structures (depots, stations, yards, plants, service buildings, docks, metro stations, cargo hubs). An implement whose roads would run over one, or leave one with no street, is refused and the project stays pending: `implemented` is false (`project implement` replies `did: false`), `implement_refused` gives the reason, and every project reply lists `blockers` (`[{id, name}]`), the structures the pending roads run over now (drawn with a wide red border in the game). Move the road or bulldoze the structure, then implement again.

### Placing buildings

`place {kind, x, y}` (or `pos: [x, y]`):

- `station`, `freight_yard`, `central_station` (also `central`, `main_station`): rail pieces, placed
  anywhere (`World::place_rail_facility`); the reply message says whether track and street are
  joined. The Central Station (one per city) is also a metro station and a bus terminal (a depot).
- `metro_station` (also `metro`, `subway_station`): a metro entrance on the lot under the point
  (`World::build_metro_station`); stations need 160 m between them. Join them with `subway {action:
  "line"}`.
- `cargo_hub` (or `hub`): a freight terminal on the lot under the point (`World::build_cargo_hub`,
  see DESIGN.md, Cargo hubs); `inspect` of it has `taxis_and_freight.cargo_hub` (loads by stage,
  the semi on its way, lorry bays, the yard or port on its siding).
- `battery_small`, `battery_medium`, `battery_large` (also `battery` with `tier` low / mid / high):
  a grid battery pack on the lot under the point (`World::build_battery`; 20 / 80 / 240 MWh, 5 / 20 /
  80 MW, cost 6,000 / 20,000 / 55,000, upkeep 30 / 90 / 240 a day, lots of 150 / 350 / 800 m², any
  land but parks). Like solar and wind, whatever stood there is cleared at once.
- `depot`, `parking`, `coal`, `solar`, `wind`, `nuclear`, `hydro`, `tidal`, `geothermal`: on the lot
  under the point, or the nearest vacant lot within 80 m (`build_depot`, `build_parking`,
  `build_power`). Hydro needs a lot within 30 m of a river or stream running at 0.8 m/s or more,
  tidal a lot on the sea shore, geothermal a lot over a hot spot (`terrain_at` says which apply).
- `shipping_port` (or `port`), `ferry_terminal` (or `ferry`): at the water's edge within 90 m of the
  point (`World::place_port`). Ports need a river at least 40 m wide or the sea; ferries any water
  16 m or wider. Each builds a pier and, unless a street is already along its landward side, a
  street stub to join to the city.
- `toll_booth` (also `toll`), `toll_plaza` (also `plaza`): across the road at the point
  (`World::place_toll`), not on a lot: the nearest street or highway piece within 15 m that has a
  straight run either side (booth 18 m, plaza 50 m between junctions or bends; a click near a
  junction slides along to fit), at least 40 m from another booth. Cost 4,000 / 15,000, upkeep 60 /
  200 a day. Reply: `toll` (the booth id, an integer), `message`, `row` (as in `tolls`), `treasury`.
- `vertiport` (also `skyport`, `drone_pad`) with `tier` low / mid / high: the air taxi base (`nd_sim::air`), on any
  lot but parks; works once the old building on the lot is cleared.
- Services and attractions: `police`, `fire`, `hospital` (`clinic` is a low-tier hospital), `school`, `jail`,
  `dispatch`, `monument`, `stadium`, with `tier` low / mid / high (default mid) or a suffix
  (`"fire:high"`), on the lot under the point (`World::place_on_lot`, `build_service`). Whatever grew
  on the lot is bought out and pulled down; another civic building refuses ("bulldoze it first").
  Refusals name the reason: land use, lot area, no street out front (police, fire, health, stadium),
  treasury. Costs, upkeep, jobs, coverage radius, vehicles and capacity per tier are in
  `nd_sim::services::ServiceKind`.

Reply: `building` id, `message`, `row`, `treasury`; or the sim's reason as `error`.

```json
{"cmd": "place", "kind": "solar", "x": -210, "y": -210}
{"cmd": "place", "kind": "fire", "tier": "high", "x": 120, "y": 40}
{"cmd": "place", "kind": "dispatch:low", "x": -80, "y": 40}
```

### Taxes and tolls

`taxes {}` reads the tax schedule; the same reply comes back from every setting form below.

| Field | Meaning |
| --- | --- |
| `set` | `{line: rate, ...}`: `line` is a key (`residential_low`, `residential_mid`, `residential_high`, `grocery`, `bakery`, `gadget_shop`, `parking`, `nightlife_low`, `nightlife_mid`, `nightlife_high`, `farming`, `oil`, `refinery`, `manufacturing`, `logistics`, `fishing`, `tech`; industry kinds also by their `industry` names) or a land use (`residential`, `commercial`, `entertainment`, `industrial`) to set its whole group. A rate is a share (`0.2`) or a percentage (`20`, `"20%"`) |
| `category` + `rate` | one line |
| `import_duty` | share added to the price of imports, 0..1 |
| `reset` | `true`: every rate back to the standard, no duty |

Reply: `lines` (per line: `key`, `label`, `land_use`, `rate`, `default`, `max` (0.40 income, 0.30
business), `pull` (1 at the standard rate; below 1 the line is taxed over it and grows less),
`buildings`, `homes`/`jobs`, `residents`/`workers`, `today` (the last midnight's collection),
`yesterday`, `recent_per_day` (running average)), `by_land_use` {buildings, today, yesterday, pull},
`import_duty`, `duty_today`, `duty_yesterday`, `standard`, `tax_today`, `changed`.

Taxes are collected at midnight: incomes at the rate of the home's tier (12% standard), business
takings at the rate of the business's line (5% standard; offices and the city's own car parks stay at
5%). The standard rates change nothing; only the difference reaches purses (a household taxed at 20%
pays the extra 8% of its income each day, a farm at 0% keeps its 5%). See `nd_sim::tax` for what
each rate pulls on. The same schedule is in `report {sections: ["economy"]}` under `taxes`.

```json
{"cmd": "taxes", "set": {"oil": 0.2, "farming": 0, "residential_high": 25}}
{"cmd": "taxes", "import_duty": 0.3}
{"cmd": "taxes", "reset": true}
```

`tolls {}` lists the booths: `booths` (id, name, size `toll_booth`/`toll_plaza`, pos, price,
truck_price, lanes, highway, segment, cars/revenue today, yesterday and total, upkeep_per_day),
`paid_today`, `free_today` (buses, emergency vehicles), `trucks_today`, `revenue_today`,
`upkeep_per_day`, the ledger's `road tolls` today and yesterday. `{action: "price", id, price}` sets a
booth's toll (0..50 per car; lorries pay double), `{action: "remove", id}` takes it away (no refund),
`{action: "inspect", id}` returns one row; `at: [x, y]` picks the booth within 30 m instead of `id`.
Booths go with their road when it is bulldozed. The Bulldoze tool removes a booth clicked within 8 m.

```json
{"cmd": "place", "kind": "toll_plaza", "x": 0, "y": 420}
{"cmd": "tolls", "action": "price", "id": 1, "price": 4}
```

### Industry sub-zones

`industry {lot | pos | polygon, kind}` sets the player's industry bias (`World::set_industry_bias`):
`kind` is `farming`, `oil`, `refinery`, `manufacturing`, `logistics`, `fishing`, `tech`, or `none`
(hand the area back to the ground over an older bias). `lot` is a lot id, `pos: [x, y]` the lot
under the point, `polygon: [[x, y], ...]` an area (the bias belongs to the ground, so it survives
the lots being recut). New industrial buildings there take that kind; standing industry of another
kind retools on the spot. Reply: `kind`, `retooled` (buildings changed), `biases`.

Without `kind` it is a query of the lot: `ground` {soil, fertility, oil, rock, sand, water},
`contamination`, `land_value`, `near` {port, yard, highway, depot (0..1), fleet_lot}, `neighbours`
([kind, metres] of industry within 160 m), `bias`, `chance` (share of each kind a new building
would draw) and `building` (the kind standing there).

```json
{"cmd": "industry", "pos": [-210, -210], "kind": "farming"}
{"cmd": "industry", "polygon": [[-280, -280], [-140, -280], [-140, -140], [-280, -140]], "kind": "none"}
{"cmd": "industry", "pos": [-210, -210]}
```

### Buses

`bus {action, ...}`:

| action | fields |
| --- | --- |
| `select` | `route` (omit to clear): highlight that line on the map, its path, stops and every bus on it, as clicking it in the Transit window or clicking one of its buses does |
| `new_route` | (none) |
| `add_stop` | `x`, `y`, `route` optional (default: the line being drawn, or a new one) |
| `move_stop` | `route`, `index`, `x`, `y` |
| `remove_stop` | `route`, `index` (the last stop removes the line) |
| `set_buses` | `route`, `n` |
| `remove_route` | `route` |
| `rename` | `route`, `name` |
| `recolour` (or `recolor`) | `route`, `color` (0..7, the line and its buses) |
| `line` | `route`: the Lines window's figures: per stop `on_today`, `off_today`, `on_yesterday`, `off_yesterday`, the leg to the next stop (`leg_m`, `leg_min`, `leg_timed`, `leg_runs`, `leg_broken`); `length_m`, `lap_min`; `buses_out` with `state` (on_line, last_lap, from_depot, to_depot), `next_stop`, `progress` (in stops: 2.4 is 40% from the third stop to the fourth), `passengers` |
| `select_stop` | `route`, `index`, `look` (bool, point the camera there): mark the stop on the map as the Lines window's "show" does; no `route` clears it |
| `list` | |

Rows (`list`, and the `row` the edits return) carry `riders_yesterday` and each stop's
`on_today`/`off_today`. The Lines window is `window {name: "lines"}`.

```json
{"cmd": "bus", "action": "add_stop", "x": 0, "y": 136}
```

### Subway

`subway {action, ...}` (also `metro`; `nd_app/src/remote/subway.rs`):

| action | fields |
| --- | --- |
| `status` (default) | the `subway` report section (below) |
| `station` | `x`, `y`: a station on the lot under the point (or the nearest usable lot within 80 m) |
| `line` | `stations: [id or [x, y], ...]` in order (a point picks the station within 120 m), or `points: [[x, y], ...]` (a station is placed on the lot at each point that has none within 120 m); optional `headway` minutes. Pays the tunnel |
| `headway` | `line`, `minutes` (4..60): minutes between trains each way |
| `remove_line` | `line`: its trains are taken off (riders set down at the next platform) and people waiting walk instead |
| `inspect_line` | `line`: the row plus `path` (every fourth point) and `trains_detail` (id, forward, at, passengers, dwelling, next_stop) |
| `inspect_station` | `station`: id or `[x, y]` |

Line rows: id, name, colour, headway_min, length_m, stations (id, name), trains, waiting, riding,
riders_today, riders_yesterday, runs_today. Station rows: id, name, central, entrance, platform,
lines, boardings_today, boardings_yesterday, waiting.

```json
{"cmd": "subway", "action": "line", "points": [[300, -440], [210, -160], [90, -25], [-90, 25], [-210, 160]], "headway": 8}
{"cmd": "subway", "action": "headway", "line": "1:1", "minutes": 6}
{"cmd": "subway"}
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

`inspect {segment: id}` or `inspect {road: [x, y]}` (the piece nearest the point) gives a road piece: ends, kind,
lanes, `level` (ground / viaduct / cutting / tunnel / bridge), `levels` and `heights` at its ends, `inner_heights` (`[t, z]` samples between them on a long street over the hills), the
`bridge`/`viaduct`/`tunnel` flags, its nodes and their degrees, and `grade_crossings`: every way it
passes over or under (`passes`, `other_segment`, `clearance`).

`inspect {junction: [x, y]}` gives the nearest junction: its signal (phases, green, timer, protected
left arrow), each arriving street's lane arrows (`"L"`, `"S+R"` ...), recent turn shares, queues and
the mean and max left turn wait there. A car's `inspect` carries `traffic` (wait reason, lane arrows,
next turn, left turn wait, passing on the shoulder). `lane_use {junction: [x, y], approach: [x, y],
lanes: ["L", "S+R"]}` sets the arrows of the street arriving nearest `approach`, leftmost lane first;
`lanes: "reset"` goes back to the demand-chosen default.

`inspect` with one of `citizen`, `building`, `lot`, `car`, `drone`, `vehicle` (a private car's record: owner, model, place with minutes_here, limit_min, illegal, ticketed_tick, impounded {pound, out_of_town, release_fee, auction_in_min} and tow_job), `train`, `route` (ids) or `gesture` (int). A building that is a parking office adds `parking_office` {officers, patrol cars}, a tow company `tow_company` {trucks, jobs_open, pound}, a lot the city runs `parking.city_run`; a tow truck's `car` adds `tow` {job, state, reason, vehicle, company, out_of_town}, an officer's car `patrol`. `parking_price {building, price}` sets the fee at a city-run lot. `place` takes `parking_enforcement[:tier]`, `parking_small`, `parking_large`, `parking_garage`;
with none, the current selection. The detail matches what the inspector and the person pane show:
A citizen's `inspect` also carries `vehicles` {bike, car {id, body, class, price, place: parked (home / building / parking lot / kerb / street (no space) / dealer, building, pos, since_tick) or driving (car id, errand)}} and `commute` {pain per mode with recent_trips, parking_pain, trip_mode_now, last_trip {mode, minutes, pain}}: what the mode choice weighs and what the daily habits (buy a bike, sell the car, buy one) act on.

- citizen: name, personality, activity (label, destination, car/bus/train ids), home/work, mood,
  satiety, class, income, rent, savings, groceries, car/bike, thought, memories (newest first)
- building: land use, tier, style (the style drawn, `nd_procgen::building_style`), floors, built day, construction (progress, days left,
  deliveries, materials), land value, rent, wage, condition, homes/jobs and the people,
  footfall, power (powered share, demand), parking, plant, depot, rail link, business (cash,
  takings, `industry`, `books`: the last 30 days' revenue and spend, oldest first, needs and sells
  with stock and prices), park
- lot: land use, polygon, area, land value and tier, zone, building, vacant/landlocked, footfall

### Selection and the player's UI

| Command | Fields |
| --- | --- |
| `select` | `citizen` / `building` / `car` / `vehicle` (a parked car) id, or `at: [x, y]` (picks like a click), or nothing to clear |
| `selected` | full detail of the player's selection (see below) |
| `flags` / `flag` | the player's "Flag for Claude" bookmarks (see below) |
| `favourite` | `citizen`, `on` (omit to toggle) |
| `tool` | `name` (inspect, road, zone, edit, bulldoze, buildings (alias power), transit, rail, district; the building ones are what the Build window's items pick: road and its presets for Roads and Rail track, zone for Zoning and Parks, buildings for Power, Services, Transit placeables, Water and Attractions, rail for stations and yards, transit for bus and metro lines), `preset` (Path, Rail, Lane, Street, Avenue, One-way, Ramp, Highway, Roundabout), `diameter` and `ring_lanes` (roundabout), `ring_at` `[x,y]` (show the roundabout preview there as if hovered, until the mouse is used), `kind` (the active tool's sub-mode: road kind, for buildings a category power / services / transit / rail / attractions or any `place` kind, station / freight_yard, route / depot, land use or zone mode), `mode` (zone: polygon / brush / marquee), `radius` (zone brush), `unzone`, `industry` (zone, industrial: auto / farming / oil / ...), `tier` (buildings: low / mid / high), `land_use`, `lanes_fwd`, `lanes_bwd`, `curved`, `show_routes`, `route`, `selected_gesture`, `elevation` (the Road tool's level for the next point, m), `depth` (underground / ground / sky: the view layer; the Tunnel preset switches to underground, other presets back), `draw` `[[x,y,level?],...]` (the road being drawn, as if clicked, so its hologram shows), `cursor` `[x,y]` (where the pointer stands for that hologram until the mouse is used). The reply's tool state has `elevation`, `elevation_label`, `point_levels`, `tunnel`, `depth`, `grade_warnings` |
| `overlay` | **Map layers are exclusive**: one shows at a time (as the top bar's Layer button), with its key in the legend. `active` a layer key or `none`: show it alone. `layer` (zoning, land_value, districts, traffic, traffic_types, paths, people, pollution, or a land survey layer, below) with `on` (bool; omitted toggles): on shows it and hides every other, off hides it if it shows. `land` a land survey layer key (shown alone) or `none`. Older form, a bool per layer name: true shows that layer alone (several trues: the last listed in the registry wins), false hides it if it shows. `projects` (bool, or `layer: "projects"`) is the dim other-projects switch, not a layer, and is left alone by the rest. Reply: every flag layer and whether it is on, `land` (the survey layer shown, or null), `active` (the key of the layer showing, or null) and `projects` (also in `status.overlays`). New layers are added as one entry in `crate::map_layers::MAP_LAYERS` and appear here by their key |
| `lighting` | `follow_clock`, `hour` (0..24; setting an hour unpins lighting from the clock) |
| `heat_vision` (alias `heat`) | `on` (bool; omitted toggles). Drone heat vision: buildings, ground and roads go near-black, people, vehicles, fires and drones glow by temperature (lights and engines white-hot), with motion blur, scanlines, noise, a vignette and a reticle. Same as key H and the top bar's Heat button. Reply `heat_vision`; also `status.lighting.heat_vision` |
| `window` | `name` city / economy / taxes / market / people / power / transit / emergencies / view (also `land`, `layers`, `map`: the View window, once Land) / districts / saves / help / dev, `open` (omit to toggle), `expanded` (bool: the large view with charts over time and the drill-downs; `true` also opens the window, and only one window is expanded at a time, so expanding one puts the other back to compact; saves, help and dev have none). Reply `open`, `expanded` (the expanded window's name or null), `windows_open`. Applied in the egui pass of the same frame, so a following `screenshot` shows it; `status.window_expanded` names the expanded window. `name: "build"`: the Build window, `open` (omit to toggle), `section` roads / zoning / parks / districts / power / services / transit / rail / water / attractions (opens it there); `status.build_window` {open, section}. A building tool picked with `tool` opens the Build window at its section, as its hotkey does. `name: "title"`: the title screen (above) |

Land survey layers (map layers like the rest, so one at a time with them; K steps through these
in the game; the top bar's Layer button and the View window list them;
`nd_sim::survey`): a coloured ground wash over the buildable square.

| Key | Shows |
| --- | --- |
| `farmland` | soil fertility, less where contaminated (what farms yield by) |
| `oil` | oil deposits (wells only go over one) |
| `geothermal` | hot spots: solid where a geothermal plant would work, a faint halo around |
| `rock` | rock and steep ground (spoils farming; nothing quarries it yet) |
| `sand` | beach sand on the sea shore |
| `water` | closeness to open water (fishing, ferries) |
| `shipping` | shore where a shipping port fits: the sea or a river 40 m wide |
| `best_industry` | each spot coloured by the industry kind the ground favours most |
| `best_farming`, `best_oil`, `best_refinery`, `best_manufacturing`, `best_logistics`, `best_fishing`, `best_tech` | the chance a new industrial lot there takes the kind, from the sim's own industry weights (the player's area bias left out) |

Aliases: `fertility`, `hot_spots`, `quarry`, `port`, `industry`, and `land_<key>`. `terrain_at`
returns every layer's value at a point under `survey`.

```json
{"cmd": "overlay", "land": "geothermal"}
{"cmd": "overlay", "layer": "best_farming", "on": true}
{"cmd": "overlay", "land": "none"}
```

### What the player has selected, and flags

`selected` returns everything about the player's current selection in one reply, as `inspect` gives
it: `kind`, `id`, `selection` (kind, id, label), `detail`, `bus_line` (the line picked in the Transit
window, with its buses) and `clock`. So when the player says "this car", ask `selected`.

- **car**: `car_detail` (below), including `why`, `jam`, `trip` and `lanes`.
- **citizen**: as `inspect {citizen}`, plus `driving_car` (the car's full detail) while they drive.
- **building**, **parked_car**: as `inspect {building}` / `inspect {vehicle}`.

A car's detail (`inspect {car}`, `selected`) carries:

- `label`: "Mara Voss's hatchback", "bus on Line 2", "delivery truck", "a sedan passing through".
- `why`: `reason` (moving, queued, letting_out, red_light, pedestrians, give_way, turning_left,
  roundabout, exit_full, lane_change, at_kerb, bus_stop, unloading, arriving, crashed, takeover,
  in_show, unexplained), `text` (plain language, naming the vehicle responsible), `by` and
  `by_label` (that vehicle: select it to follow the queue), `good_reason` (false only for
  `unexplained`: a car standing for no reason the sim knows, a bug worth reporting), `stalled_s`.
- `jam`: who it waits for, back to the head of the queue (`Traffic::jam_chain`): `length`,
  `cycle` (a gridlock ring), `head`, `head_label`, `head_reason` (a `why`) and `chain` (each car's id,
  label, reason, speed, pos, stalled_s; at most 60).
- `trip`: `elapsed_min` on the road vs `expected_min` (from the route at departure, free flow plus
  a little per junction), `over_ratio`, `can_rage` (private drivers), `anger` and `anger_peak`
  (0..1), `patience`, `temper`.
- `driver`: for a private car `citizen`, `name`, `age`, `mood`, `class`, `purpose`, `destination`,
  `destination_name`, `anger`; for every vehicle `style` {`label`, `boldness` (-1 slow .. 0 normal ..
  1 reckless, the inspector's slider), `calm_boldness`, `skill`, `speed`, `headway_s`, `aggression`,
  `compliance`, `drunk`}.
- `occupants`: everyone aboard (`id`, `name`): the driver of a private car, a bus's riders.
- `lanes`: `lane` (id, road segment and index, or junction node and turn), `s_m`, `s_end_m`,
  `route_idx`, `route_len`, `ahead` (the next 12 lanes), `route_lanes` (ids from here), `ends_at`,
  `entering_from_kerb`, `blocked_by`.

**Flag for Claude.** In game, F8 (or the "Flag for Claude" button in the inspector and the person
pane) opens a note box; saving records the selection (or, with nothing selected, where the camera
looks) with the note. `flags {since?, clear?}` lists them: `id`, `tick`, `time`, `unix`, `note`,
`selection`, `look_at`, `snapshot` (the `selected` reply when flagged: a stuck car may have moved on
since) and `now` (its live detail, or null once it is gone). `flag {note}` flags the current
selection from the API. Flags live in the running game and are also appended to
`flags/claude_flags.jsonl` under its working directory.

```json
{"cmd": "selected"}
{"cmd": "flags", "since": 3}
{"cmd": "flag", "note": "this one has not moved for ages"}
```

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
`<working dir>/screenshots/shot-<unix seconds>.png`). A given path must end in `.png` and lie in
the allowed folders (see [Who may call](#who-may-call)). **The file is written a frame or two later**,
when the GPU copy lands: poll for it (the MCP tool waits up to 5 s). It captures the whole window,
UI panel included, at the window's size (a full-HD window makes a 2–5 MB PNG). PNG only. A
minimised window renders nothing and produces no file.

### Events

`events {since?, limit?}`: a ring buffer of the last 2000 events, each with a running `seq` and the
`tick`. Kinds:

- city: `NewDay`, `RoadsChanged`, `LotsChanged`, `PlanChanged`;
- buildings: `BuildingBuilt`, `BuildingChanged`, `BuildingRemoved`, `Expropriated`, `BusinessClosed`,
  `StadiumEvent`;
- people: `CitizenArrived`, `CitizenLeft`, `NewThought` (with the thought text);
- emergencies: `IncidentStarted`, `EmergencyCall`, `UnitDispatched`, `UnitOnScene`,
  `IncidentResolved` (each with `incident`, `type`, `building`, `name`, `outcome`), `FireSpread`,
  `BurnedDown`, `Arrested` (incident, citizen), `Released` (citizen);
- driving: `Crash` (crash id, pos, cause, severity, injured, place, incident), `Takeover`,
  `TakeoverOver`;
- the UI: `Message` and `ToolStatus` whenever the status lines change. Poll with `since` = the last `seq` you saw; `dropped: true` means events fell out of the
ring in between. `thoughts {limit?}`: the newest memories across all citizens, and current thoughts.

## Reviewing the city

Five read-only commands answer "how is this city doing, where, and why" in a few calls, with
compact, labelled JSON (units in the names: `_per_day`, `_m`, `_mw`; a `share` is 0..1). The code
is in `crates/nd_app/src/remote/{report,map,history,list}.rs`.

**Recommended sequence for an LLM review**

1. `status`: clock, money, and `alerts_top`, the five worst problems.
2. `report`: the whole city in one reply (about 25 KB for a 1,700-citizen city). Read `alerts` first.
3. `map {layer: "hunger"}`, `{layer: "land_value"}`, `{layer: "population"}`: where. The ASCII heat
   map shows clusters; the alerts' `where` fields point into it.
4. `history`: getting better or worse (`trend` per key).
5. `list` to rank (e.g. businesses by `staffing`), then `inspect` the ids it returns.

Each reply carries `took_ms`; see the timings measured on a real city at the end of this section.

### Power

`report {sections: ["services"]}` has `power` (`remote/report.rs::power_json`): `supply_mw`,
`demand_mw`, `imported_mw`, `exported_mw`, `curtailed_mw` (surplus with nowhere to go), `served_share`,
`unpowered_buildings`, `brownout_hours_today`, `upkeep_per_day`, `import_cost_today`,
`export_income_today`, `import {max_mw, price_per_mwh}`, `export {mw, max_mw, price_per_mwh,
income_today, income_yesterday}`, `battery_totals {count, charge_mwh, capacity_mwh, level, charging_mw,
discharging_mw, round_trip_efficiency}`, `weather`, and a row per plant and battery
(`World::plant_rows`, the same figures as the Power window):

- `plants`: id, name, kind, capacity_mw, available_mw (what the weather allows), output_mw (less
  curtailment), mean_mw_24h, upkeep_per_day, cost_per_mwh (upkeep over the last 24 hours' energy; null
  when it made next to nothing), co2_t_per_day, co2_t_per_mwh, air_index (smog at the fence now, 0..1),
  noise_db (null while idle), noise_db_running, pollution_radius_m, pollution_land_value_effect,
  workers, jobs, derelict, pending (lot still being cleared), pos, `output_mw_hours` (the last 72
  hours) and `energy_mwh_days` (up to 90 days).
- `batteries`: id, name, kind, capacity_mwh, max_mw, charge_mwh, level, flow_mw (+ delivering,
  - charging), state (charging / discharging / idle), delivered_mwh_24h, upkeep_per_day,
  cost_per_mwh, noise_db, derelict, pending, pos, `flow_mw_hours`, `charge_mwh_hours`.

`status.power` carries the headline figures plus `exported_mw`, `export_income_today`, `batteries
{count, charge_mwh, capacity_mwh, charging_mw, discharging_mw}` and `rows {plants, batteries}` (the
rows without the hourly series). `report {sections: ["history"]}` hours gain `exported_mw`,
`curtailed_mw`, `battery_in_mw`, `battery_out_mw`, `battery_charge_mwh`, `battery_capacity_mwh`; days
gain `exported_mwh`, `battery_in_mwh`, `battery_out_mwh` and `ledger.grid_export`.

### report

`{"cmd": "report", "sections": ["overview", "alerts"]}` (default: all but `history`).
`{"cmd": "report", "sections": ["history"], "history_days": 90}` for the sim's history rings.

| Section | Fields |
| --- | --- |
| `overview` | day, time, population, households_housed, homes {total, free, being_built}, jobs {total, filled, free, being_built}, buildings {built, under_construction, derelict, vacant_lots, unusable_lots, lots_total}, treasury, net_yesterday, net_today_so_far, land_value_avg, gini, demand_by_land_use (0..1), headline_alerts |
| `people` | by_class, by_activity, employment {employed, employed_in_businesses, employed_on_building_sites, unemployed_on_gig_income, unemployment_share (of everyone), jobs_free, working_age_adults, unemployed_adults, adult_unemployment_share}, housing {housed, unhoused, waiting_immigrants, homes_free}, income_per_day and savings (distributions; savings also in_overdraft, at_overdraft_floor), rent_burden {over_30pct_of_income, over_50pct_of_income, their shares, rent_stressed_now, rent_to_income distribution}, mood (distribution 0..1), hunger {hungry_now, mean_satiety, no_groceries_at_home, hungry_days histogram 0..4+}, commute_modes_now (walk/bike/car/bus/train counts and shares of the people travelling now; owns_car_share, owns_bike_share), moves (displaced, emigrated, priced-out totals), recent_memory_reasons (memories of the last 2 days grouped by their fixed text, e.g. "Rent got too steep", "Couldn't afford anything at") |
| `economy` | treasury, ledger {today_so_far and yesterday by flow, nets}, ledger_items {yesterday, today_so_far: per flow {total, items: [{building + name / route + name / class / land_use / name, amount, postings}], not_itemised}} (the Economy window's drill-down: service upkeep by station, clinic and car park, transit upkeep by depot and the bus fleet, plant upkeep by plant, fares by line and station, income tax by class, business tax by land use), tax_today, upkeep_today, trade_today {imports_value, exports_value, building_materials_imported, deals, meals_sold, walk_in_sales, parking_fees}, businesses {total, open, closed_ever, by_kind: count, open, closed, mean_staffing, mean_money, revenue and spend totals, inputs (stock, target, fill), outputs (stock, mean_price)}, resources (town_stock, avg_price, price_vs_base, local_sellers_open, local_supply_per_day, business_demand_per_day, scarce), most_profitable_today and least_profitable_today (10 each, with ids) |
| `land` | lots_by_land_use {vacant, built, under_construction, unusable, total, area_m2}, buildings_by_land_use_and_tier, special_buildings, floors_histogram, rents_by_tier {buildings, homes, residents, occupancy, rent_per_home_day_avg, min, max}, land_value {avg_grid, lots distribution, tier_cuts}, parks {lots, area_m2} |
| `services` | emergency {coverage_pct_of_homes {police, fire, health, school}, incidents_today {fire, crime, medical}, resolved_today, unanswered_today, arrests_today, avg_response_min (call to arrival, today or else yesterday), response {fire, crime, medical: answered, median_min, p90_min, within_30_min_pct, of (today or yesterday)}, queued {fire, crime, medical} (calls past their dispatch time waiting for a unit), queue_max_today, all_units_busy_min_today, all_units_busy_now_min (null when a unit is free), unanswered_why_today {per kind: all_units_busy, no_route, no_station}, mutual_aid_today, unreachable_today, diverted_today, units_on_patrol (crashes count with police in the per-kind figures), units, units_busy (on a call: responding, on scene, transporting), fires_burning, burned_down_total, waiting_for_ambulance, in_hospital, dispatch {dispatcher_on_duty, minutes_to_dispatch}, stadium_crowd_today, tickets_today, stadium_events_tonight, upkeep_per_day, education_wage_bonus_today, stations: id, kind, tier, workers, radius_m, units, units_crewed, units_busy, units_on_patrol, beds_used, capacity; active_incidents: id, kind, stage, building, minutes_since_start, intensity, unit, dispatcher; recent (last 10 resolved with outcome and response_min)}, power (see Power below), transit {routes: id, stops, buses_wanted, buses_running, riders_today, waiting, length_m, broken_legs, runnable; totals}, subway {stations, lines, trains, tunnel_m, waiting, riding, riders_today, riders_yesterday, fares_today, fares_yesterday, upkeep_yesterday, runs_today, fare, in_service, line_list (line rows as for the `subway` command), station_list (station rows)}, rail {facilities: id, kind, rail_connected, road_connected, works, waiting_passengers, freight units; trains by kind; riders, freight}, taxis {companies: id, name, fleet, cabs (id, number, place yard/rank/kerb/road, job, on_duty, rider, car, rank, pos), rides_today, missed_today, money; cabs, on_duty, in_yard, at_ranks, at_kerb, on_road, carrying, to_pickup, riders_waiting, rides_today/yesterday, fares_today/yesterday, mean_wait_min_today, mean_ride_m_today, drunk_rides_today, night_rides_today, gave_up_today/yesterday, unserved_demand_today/yesterday, companies_founded_total, cabs_bought_total, cabs_sold_total}, cargo_hubs {hubs: id, name, works, units {waiting_for_semi, on_semi, inbound_rail_or_sea, in_shed, exports_gathered}, loads, semi_on_the_way, lorry_bays, lorries_parked, siding_yard, quay_port; received_today, forwarded_today, semis_in_today, semis_out_today, long_haul_trips_saved_today/yesterday/total, handling_fees_today}, air {vertiports, pads {vertiports, rooftop, hospital, fire, police}, rooftop_pads, drones {taxis, air_ambulances, police_helicopters, airborne, holding, mean_taxi_battery}, flights_today, riders_today, fares_today and yesterday's, waiting_for_drone, in_the_air, gave_up_today, holds_today, medic_flights_today/total, police_flights_today, company {money, revenue}, noise {cells, max, max_land_value_penalty, grumbles_today}}, parking {capacity, used, occupancy, parked_on_street_today, last_shortage_at, enforcement {rule, offices: id, name, tier, officers, patrol_cars_now, patrol_cars_wanted, radius_m; patrol_cars_now, coverage {kerb_slots, within_patrol_reach, share}, tickets_today, fines_today, tickets_total, fines_total, on_street_now {over_limit, ticketed}, tows_today, tows_out_of_town_today, tows_total, tows_out_of_town_total, tow_jobs_open, tow_trucks_on_road, wrecks_towed_total, pound {cars, in_town, out_of_town, released_today, release_fees_today, released_total, auctioned_today, auctioned_total}, tow_companies: id, name, staff, trucks, in_pound, money; city_lots: id, name, size, capacity, used, occupancy, price; city_lot_fees_today, parked_by_place}}, traffic {vehicles, cars, trucks, buses, mean_speed_kmh_moving, stopped, stuck, waiting_at_kerb, gridlocks, left_turn_wait_mean_s, left_turn_wait_max_s (over the last 500 left turns), left_turns_measured, protected_left_arrows_now, passing_left_turners_now, lane_use_set_by_player, lanes, intersections, signals, private_cars {cars, cars_parked, cars_driving, cars_searching (on the road looking for a space), parked_by_place (home, building, parking lot, kerb, street (no space), dealer), kerb_slots {total, used, occupancy}, created_total, destroyed_total, events_total (arrived_with_household, bought, bought_out_of_town, sold, scrapped, emigrated: the only ways a car comes or goes), bought_today, sold_today, bikes_bought_today, bikes_sold_today, circled_today, search_minutes_today, parked_today {home_or_building, lot, kerb, no_space}, mode_shares_today and mode_shares_yesterday {trips, modes {walk, bike, bus, train, car, taxi: trips, share}}, commute_pain {per mode: users (who used it lately), mean (bad half-hours per trip)}, parking_pain_mean_of_owners, fleet_parked_at_businesses, fleet_bought_total, fleet_returned_today}} |
| `industry` | by_kind (farming, oil, refinery, manufacturing, logistics, fishing, tech): buildings, open, jobs, mean_staffing, profit_today, nuisance {radius_m, weight}, fouls_ground; contamination {cells, cells_over_0_3, max, total, cell_m}; fleet {truck_and_bus_lots, bays, trucks_parked, trucks_on_road, returning_empty}; biases {count, newest_first}; plant_land_value (each plant kind's radius, weight, load, ash) |
| `alerts` | the ranked list below |
| `history` | only when asked for by name: the sim's own history (`nd_sim::history`, kept in the save, unlike the `history` command's app-side rings) as columns, oldest first. `days` (the last `history_days`, default 30, max 365): day, treasury (after midnight upkeep), net, income, spending, `ledger.<flow>` for every flow, `income_tax.<low/mid/high>`, `business_tax.<land use>`, population, households, working_age, employed, unemployed, unhoused, waiting_immigrants, `classes.<class>`, homes, homes_filled, jobs, jobs_filled, buildings, sites, avg_land_value, gini, `avg_rent.<tier>`, `demand.<land use>`, businesses, imports, exports, materials, deals, `supply_mwh.<plant kind>`, demand_mwh, imported_mwh, peak_demand_mw, brownout_hours, bus_riders, rail_riders, ferry_riders, air_riders, `route_riders.<line name>`, `incidents.<fire/crime/medical>`, `resolved.<kind>`, unanswered, out_of_town_calls, avg_response_min. `hours` (the last 72): day, hour, treasury, population, supply_mw, `supply_mw.<plant kind>`, demand_mw, imported_mw, served, bus_waiting, bus_riding, rail_riding, cars. Also `days_kept` |

A distribution is `{count, mean, median, p10, p90, min, max, histogram: {edges (bins + 1), counts}}`
(16 bins over the data's range for money, 10 over 0..1 for shares and mood).

**Alerts** are ranked by severity (high, medium, low), then by count. Each has `kind`, `count`,
`message`, `where` (the centroid [x, y] of the cases), `spread_m` (their mean distance from it:
small means one hot spot), `example_ids` (up to 5, for `inspect`) and a `hint`. Kinds:
starving_citizens, hungry_citizens, unhoused_citizens, rent_stressed_residents,
food_sellers_understaffed, food_sellers_empty, closed_businesses, broke_businesses,
unpowered_buildings, overcrowded_homes, rail_facility_not_connected, bus_route_broken,
construction_waiting_for_materials, stuck_vehicles, derelict_buildings,
vacant_lots_with_high_demand, high_unemployment, treasury_losing_money, parking_shortage,
scarce_resources, fires_burning, no_police_coverage, no_fire_coverage, no_health_coverage,
no_school_coverage (residential buildings outside a service's road reach, once there are 10+),
slow_emergency_response (average over 45 minutes from call to arrival), no_dispatcher (calls go to
the regional line), emergency_units_saturated (every police car, engine or ambulance was out on
calls for an hour or more today; the message names the bottleneck, e.g. "all 2 police cars busy for
3.0 h today (up to 4 calls waiting at once, 1 unanswered calls today because every unit was out):
build another police station or upgrade the tier", or that only some vehicles have a crew),
emergency_calls_unreachable (a unit was free but no road led to the call). The thresholds are named constants at the top of `remote/report.rs`: hungry =
satiety < 0.3; starving = 2+ hungry days (the sim sends people away at 4); a food seller is
understaffed below 50 %; unpowered = under 60 % of demand met; rent stress = rent over 50 % of
income; stuck = in traffic and standing still for 180 s (not a bus at a stop, not at a red light;
cars waiting at the kerb to pull out are counted apart, and gridlock rings are listed with the
junctions they pass through); high demand ≥ 0.6; unemployment
≥ 15 % of working-age adults (children and retirees are not counted); scarce = average price ≥ 1.3 × base.

Trimmed real reply (day 10 of the demo district):

```json
{"alerts": [
  {"rank": 1, "kind": "starving_citizens", "severity": "high", "count": 354,
   "message": "354 citizens have gone 2+ days without a proper meal", "where": [45.95, 14.2], "spread_m": 201.0,
   "example_ids": ["4:1", "5:1", "14:1", "16:1", "18:1"], "hint": "these leave the city at 4 hungry days; ..."},
  {"rank": 2, "kind": "unpowered_buildings", "severity": "high", "count": 100,
   "message": "100 buildings get less than 60% of their power (served 89% of demand)", "where": [111.58, -32.8], "...": "..."},
  {"rank": 4, "kind": "hungry_citizens", "severity": "medium", "count": 487, "...": "..."}],
 "overview": {"day": 10, "population": 1712, "treasury": 152345.67, "net_yesterday": 22434.52, "gini": 0.27, "...": "..."},
 "took_ms": 2.2}
```

### map

`{"cmd": "map", "layer": "hunger", "cell_m": 100}`. The occupied bounding box (every lot) is cut
into square cells of `cell_m` (default 100 m, at least 25), grown until there are at most 400.
Reply: `cell_m`, `cols`, `rows`, `x_range`, `y_range` and `cell_coords`, the formula: the centre of
cell (col, row) is `[x0 + (col+0.5)*cell_m, y0 + (row+0.5)*cell_m]`; row 0 is the smallest y, at
the top; col 0 the smallest x, at the left.

- `layer: "summary"` (default): `columns` and `cells`, one row per non-empty cell: col, row,
  center, residents (counted at their home), homes, jobs, jobs_filled, businesses, land_value (lot
  average), hungry, vacant_lots, unpowered, cars (now), land_use (dominant by lot area).
- One of `population`, `people` (citizens where they are now, indoors at their building: the
  People map layer's data), `land_value`, `jobs`, `hunger`, `vacancy`, `power`, `traffic`, `fertility`,
  `oil`, `contamination` (the ground at each cell centre, 0..1), `industry` (industrial buildings): `unit`,
  `max`, `total`, `grid` (rows of numbers), `ascii` (one string per row, one character per cell
  from `" .:-=+*#%@"` scaled to the layer's max; any non-zero value shows at least `.`), `legend`
  (the value range of each character) and `ascii_text` (framed, with each row's y):

```
hunger (max 45.00 per 100 m cell; ' ' = 0)
+------+
|      | y=-335
|   =@*| y=-235
|   #@*| y=-135
|    -+| y=-35
|=-    | y=65
|#+= +*| y=165
|*+= =+| y=265
+------+
 x -300 .. 300 left to right
```

### history

`{"cmd": "history", "keys": ["population", "hungry"], "days": 30}`. Two rings kept **by the
running app, not in saves**. They start empty when the game starts. They start over whenever the
city is replaced (a load or a new game, from the API or the UI), and when the clock goes backwards
or jumps more than two days at once.

- `daily` (up to 365 days): day, population, households, homes_total, jobs_total, jobs_filled,
  treasury, net_yesterday, tax_yesterday, gini, avg_land_value, mean_mood, hungry, unemployed,
  unhoused, waiting_immigrants, businesses_open, businesses_closed, buildings, under_construction,
  power_served, unpowered_buildings, transit_riders_yesterday, rail_riders_yesterday,
  meals_sold_yesterday, deals_yesterday, emigrated_total, displaced_total, priced_out_total. A
  sample is taken at the first observation after midnight (plus one at start or load); the
  `*_yesterday` counters are read just before midnight resets them.
- `hourly` (last 48 hours): day, hour, cars_on_road, trucks, buses, hungry, treasury,
  waiting_for_transit, unpowered_buildings.

Both come as `{samples, series: {key: [...]}, trend: {key: {first, last, change, min, max}}}`. The
game samples every frame (in `SimSet::Sync`), and `step` runs hour by hour so a multi-day step
still records every day and hour.

```json
{"daily": {"samples": 10, "series": {"day": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
   "population": [0, 0, 26, 212, 497, 994, 1483, 1751, 1784, 1712],
   "hungry": [0, 0, 0, 28, 60, 163, 259, 594, 699, 768]},
  "trend": {"population": {"first": 0, "last": 1712, "change": 1712, "min": 0, "max": 1784}}},
 "hourly": {"samples": 48, "...": "..."}, "started": {"why": "app start", "day": 1}}
```

### list

`{"cmd": "list", "what": "businesses", "sort": "staffing", "desc": false, "filter": {"kind": "grocery", "staffing": {"max": 0.5}}, "fields": ["name", "staffing", "money"], "offset": 0, "limit": 50}`

`what`: businesses (staffing, workers, jobs, money, revenue/spend/profit_today, deals, broke_days,
closed, powered, stock by resource, inputs_short, pos, ...), buildings, citizens (class, activity,
income, savings, rent, rent_share, mood, satiety, hungry_days, home, work, ...), lots, routes,
plants, facilities. In a filter a string matches as a case-insensitive substring, `{min, max}` as a
numeric range, anything else by equality. Sorting puts nulls last; `id` is always kept. Reply:
`total` (after filtering), `returned`, `columns` (every column of that kind), `rows`. Default limit
50, max 1000. An unknown column is an error that lists the valid ones.

```json
{"what": "businesses", "total": 139, "rows": [
  {"id": "327:1", "name": "Court 25", "kind": "office", "staffing": 0.139, "workers": 5, "jobs": 36, "money": 1590.0, "closed": false}, "..."]}
```

### export

`{"cmd": "export", "path": "exports/optional.json"}` writes `report` (all sections), `map_summary`,
`history` and `tables` (every row of every `list` kind) as one JSON document, by default to
`<working dir>/exports/city-<unix>.json` (`/exports` is git-ignored). A given path must end in
`.json` and lie in the allowed folders (see [Who may call](#who-may-call)). The file is written
atomically. Reply: `path`, `bytes`, `took_ms`.

### Timings

Release build, demo district at day 9: 1,706 citizens, 358 buildings, 144 businesses (`took_ms`,
the time spent on the main thread inside the frame; best and worst of five calls):

| Command | took_ms |
| --- | --- |
| `report` (all sections) | 2.2 to 2.8 |
| `map` (summary, or one layer at 50 m) | 0.1 to 0.2 |
| `list businesses` | 0.5 |
| `list citizens`, sorted, limit 1000 | 6 to 8 (mostly building 1,700 JSON rows) |
| `export` (900 KB file) | 21 |
| `status` | adds the alert pass for `alerts_top`, a fraction of `report`; 25 ms HTTP round trip (one frame) |

## Caveats

- **Localhost only**, no authentication: any program on the machine can drive the game. Web pages
  cannot (see [Who may call](#who-may-call)). Do not forward the port. Set `ND_REMOTE=0` to turn
  it off.
- One frame of latency per request. A command not taken within 5 s is withdrawn (HTTP 504), e.g.
  while the window is minimised and frames stall.
- Remote edits go through the same `World` calls as the tools, so they behave identically,
  including undo within a project, costs and refusals.
- `load` replaces the city: all ids from before are invalid.

## Services and emergencies

`incidents {active?, limit?, kind?}` returns the record, newest first (up to 500 kept): id, kind,
title (fire, robbery, theft, assault, vandalism, medical), stage, outcome, place, pos, the ticks it
started / was called in / dispatched / arrived / resolved, call_to_dispatch_min, dispatch_to_scene_min,
on_scene_min, call_to_arrival_min, unit ("Engine 2, Eastgate Fire HQ" or "fire engine from out of
town"), out_of_town, fee, people (citizen, name, role caller / dispatcher / victim / suspect /
patient, taken_to), damage (building, name, condition_lost, destroyed); plus jail_occupancy,
held_at_stations, released_early_total and today's out-of-town calls and fees. The Emergencies
window (`window {name: "emergencies"}`) shows the same, live.

Dispatch: calls past their dispatch time form one queue, most urgent first (fires and violent
crime: robbery, assault; then medical, crash victims included, and crash scenes; then theft,
vandalism and street takeovers), each sent the nearest free unit
the tick one is free. Free means in the station with a crew to spare, on patrol (police) or standby
(ambulances: parked near where people live), or driving home; units are sent from where they are,
may turn round in the street, and with lights and sirens pull straight out from the kerb. A violent
crime with every car busy pulls a car still on its way to a theft or vandalism (that call goes back in
the queue). A call waiting 40 minutes with every unit of the kind busy goes to the next town as
mutual aid (fee as out of town, there 20 minutes after; `unit` "... from the next town (mutual
aid)"). A call no free unit has a road to (a crash on the highway beyond the city's streets) goes
to the next town the same way at once (`unreachable_today`). Air ambulances take their critical
patients before the queue runs. Round-the-clock posts keep all three shifts manned (hiring and an hourly swap), and their
night crews keep night hours.

`incident {kind, building | citizen}` starts one by hand (fire, crime or medical) for testing; reply
`incident` (its id, as in the report and the events). `incidents {kind}` filters by fire, crime or
medical (crash victims are medical incidents).
`incident {kind: "crash", car? | at?: [x, y], speed? (m/s, default 25 = written off)}` crashes a car
(the next tick turns it into injuries and 911 calls); `incident {kind: "takeover", at?: [x, y]}`
starts a street takeover at the junction nearest `at`.

**Driving** (`report.traffic`): crashes_today, injuries_today, drunk_drivers_now, drunk_trips_today,
red_lights_run_today, crashes_total, wrecks_on_road, tow_trucks_now, towed_total, drivers_by_style,
recent_crashes (cause, severity, injured, drunk, incident), takeovers {active, location, place,
stage, participants, residents, outsiders, in_show, crowd, incident, planned_tick, total, last}.
Road rage: a private driver on the road far longer than the route should take
(`traffic::road_rage`: from about 1.4 to 3.4 times the expected minutes plus five, by patience and
temper) gets angry; anger is laid over their profile (`DriverProfile::angry`: shorter headway,
harder acceleration, more lane changes and overtaking, fewer stops at ambers, a little less skill),
builds over the trip, leaves with the car on arrival and adds to that trip's pain (a memory "Lost my
temper in traffic"). `report.traffic` adds `angry_drivers_now` and `road_rage_today`.
A car's `inspect` `traffic` carries `driver` (style, speed_factor, headway_s, reaction_s,
signal_compliance, aggression, skill, drunkenness), driver_drunkenness_now, weave_m, running_red,
crash, takeover and stunt. Events: `Crash`, `Takeover`, `TakeoverOver`.

Events in the log (`events`): `IncidentStarted`, `EmergencyCall`, `UnitDispatched`, `UnitOnScene`,
`IncidentResolved` (each with `incident`, `type` fire / crime / medical, `building`, `outcome`),
`FireSpread`, `BurnedDown`, `StadiumEvent`, `Arrested` (incident, citizen), `Released` (citizen). Building rows carry `service` (`"fire:mid"`) and
`burning`. Emergency vehicles are cars in `find {what: "cars"}`; the units behind them are in
`report {sections: ["services"]}` under `emergency.stations` and `emergency.active_incidents`.

## Concerns and happiness

`concerns {days?, concern?, quotes?}`: what people complain about. Each person counts once a day
per concern. Fields:

- `days`: the window for the daily counts and happiness, default 30.
- `concern`: only one of housing, food, jobs, safety, trash, noise, commute, parking, power,
  services, nightlife, displacement, other.
- `quotes`: recent quotes per concern, default 5.

Reply:

- `concerns`, ranked by people this week. Each has `concern`, `label`, `this_week`, `last_week`,
  `today`, `all_time`, `trend`, `direction` (rising / falling / steady), `mean_severity`, `where`
  {at, complaints, near, building} (the hot spot), `quotes` (citizen, name, text, day, severity,
  building, at) and `daily` (counts over `days`).
- `categories` (key, label, about).
- `happiness_now`, `happiness_daily` and `happiness_hourly` (48 h). Each sample has day, tick,
  people, mean, unhappy_share, happy_share and by_class.

`report.people.concerns` has the top six concerns. The `top_concern` alert names the leading one.
`window {name: "people", tab: "concerns"}` shows them in game.

## Districts and tourism

`district {action, ...}` (also `districts`):

| action | fields | reply |
| --- | --- | --- |
| `list` (default) | | `districts`: id, name, policy, residential, commercial, industry, historic, landmark, area_m2, centroid, lots, vacant_lots, buildings, under_construction, by_land_use, by_tier, in_look (buildings already in the district's look), homes, residents, jobs, workers, businesses_open, hotels, beds, tourists_staying, land_value_avg, rent_avg, attraction |
| `create` | `name` optional (generated otherwise) | `id`, `name` |
| `paint` | `id` (or `district`: id or name; none starts a new district, `new_name`), then `at` `[x,y]` + `radius` (default 40), `polygon`, or `rect` `[[x0,y0],[x1,y1]]`; `erase` | `id`, `cells_changed` (16 m cells) |
| `erase` | `at` + `radius`, `polygon` or `rect` | `cells_changed` |
| `set` | `id` / `district`; any of `residential` kowloon / suburban / eco / none, `commercial` neon_strip / tech_campus / night_market / tourist / none, `industry` farming / oil / refinery / manufacturing / logistics / fishing / tech / none, `historic` bool, `rename` | `id`, `name`, `policy` |
| `remove` | `id` / `district` | `removed` |
| `at` | `at` `[x,y]` | the district there |
| `select` | `id` / `district` (none clears) | the District tool's selected district |
| `layer` | `on` (omit to toggle) | |

Every reply carries `layer_on`; painting turns the layer on. `overlay {layer: "districts"}` toggles it
too, and `tool {name: "district"}` picks the tool. Building `inspect` carries `look` (the district
architecture it was built in) and `district`.

```json
{"cmd": "district", "action": "paint", "rect": [[-200, -150], [0, 150]], "new_name": "Lantern Row"}
{"cmd": "district", "action": "set", "district": "Lantern Row", "commercial": "neon_strip", "residential": "kowloon"}
{"cmd": "district", "action": "set", "id": 2, "historic": true}
```

`tourism {action?, limit?}` (also `tourists`): `attractiveness`, `demand_per_day`, `ways_in` (road,
rail, ferry, air), `tourists` (in town), `visible` (out on the street), `hotel_demand`, `today`
{arrivals, arrivals_by_mode, departures, turned_away, nights, sights_visited, spend, spend_hotels,
spend_shops, spend_venues, tourist_tax}, `yesterday`, `totals`, `ledger_tourist_tax`, `hotels` (id,
name, tier, beds, guests, money, revenue_today, district, pos), `attractions` (kind, name, score, pos,
building; strongest first) and `tourist_list` (`limit`, default 20: id, pos, activity, visible, class,
via, hotel, money, spent, sights, arrived_day, leaves_day, going_to). `action: "refresh"` re-reads
the attractions now instead of on the two-hour cycle. `status` carries a `tourism` summary and the
number of `districts`; the ledger has a `tourist tax` row.

## Crime, homelessness and trash

`report {sections: ["society"]}`: `criminals`, `crimes_today` (theft, burglary, assault, murder,
vandalism, total), `arrests` (total), `reoffend_rate` (released offenders who struck again),
`homeless` (people), `camps`, `rvs`, `trash` {produced, collected_city, collected_private (kg today,
plus `_yesterday`), overflowing_buildings, stored_kg, covered_buildings, stops_today,
complaints_yesterday, trucks {city, private, on_road}, depots, private_hauler {company, clients,
revenue}}, and the details under `crime` (hot spots, custody, releases, reformed, stolen) and
`street` (camp and RV lists, evictions, rehoused, outreach). Alerts: `crime_hotspot`, `no_jail`
(5+ criminals and no jail), `growing_camps`, `overflowing_trash`. Map layers `crime` (heat),
`homeless` (people in camps and vehicles), `trash` (summed overflow).

`society {action}`: `report` (default) returns the section; `evict {building}` puts its households
on the street (camps form on cheap land); `offend {citizen, building?, kind?}` makes a citizen a
criminal and has them strike; `trash {kg, building?}` adds trash to bins. `place {kind:
"trash_depot"}` (also `trash`, `garbage`) puts down a depot (industrial or civic land, a street out
front). Garbage trucks are cars in `find {what: "cars"}` with a garbage truck body.
