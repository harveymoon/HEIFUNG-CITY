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
| `status` | | clock, paused, speed, effective_speed, population, buildings, under_construction, lots, homes/jobs, treasury, ledger_yesterday, power, weather, land, market, demand, traffic, transit, rail, render (fps, ticks_per_frame, triangles), camera, tool, selection, project, overlays, lighting, message, tool_status, event_seq, favourites, `windows_open`, `alerts_top` (the first 5 of `report`'s alerts) |
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
| `window` | `name` city / economy / market / people / power / transit / land / saves / help / dev, `open` (omit to toggle). Reply `open`, `windows_open`. Applied in the egui pass of the same frame, so a following `screenshot` shows it |

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

### report

`{"cmd": "report", "sections": ["overview", "alerts"]}` (default: all six).

| Section | Fields |
| --- | --- |
| `overview` | day, time, population, households_housed, homes {total, free, being_built}, jobs {total, filled, free, being_built}, buildings {built, under_construction, derelict, vacant_lots, unusable_lots, lots_total}, treasury, net_yesterday, net_today_so_far, land_value_avg, gini, demand_by_land_use (0..1), headline_alerts |
| `people` | by_class, by_activity, employment {employed, employed_in_businesses, employed_on_building_sites, unemployed_on_gig_income, unemployment_share, jobs_free}, housing {housed, unhoused, waiting_immigrants, homes_free}, income_per_day and savings (distributions; savings also in_overdraft, at_overdraft_floor), rent_burden {over_30pct_of_income, over_50pct_of_income, their shares, rent_stressed_now, rent_to_income distribution}, mood (distribution 0..1), hunger {hungry_now, mean_satiety, no_groceries_at_home, hungry_days histogram 0..4+}, commute_modes_now (walk/bike/car/bus/train counts and shares of the people travelling now; owns_car_share, owns_bike_share), moves (displaced, emigrated, priced-out totals), recent_memory_reasons (memories of the last 2 days grouped by their fixed text, e.g. "Rent got too steep", "Couldn't afford anything at") |
| `economy` | treasury, ledger {today_so_far and yesterday by flow, nets}, tax_today, upkeep_today, trade_today {imports_value, exports_value, building_materials_imported, deals, meals_sold, walk_in_sales, parking_fees}, businesses {total, open, closed_ever, by_kind: count, open, closed, mean_staffing, mean_money, revenue and spend totals, inputs (stock, target, fill), outputs (stock, mean_price)}, resources (town_stock, avg_price, price_vs_base, local_sellers_open, local_supply_per_day, business_demand_per_day, scarce), most_profitable_today and least_profitable_today (10 each, with ids) |
| `land` | lots_by_land_use {vacant, built, under_construction, unusable, total, area_m2}, buildings_by_land_use_and_tier, special_buildings, floors_histogram, rents_by_tier {buildings, homes, residents, occupancy, rent_per_home_day_avg, min, max}, land_value {avg_grid, lots distribution, tier_cuts}, parks {lots, area_m2} |
| `services` | power {supply, demand and imported MW, served_share, unpowered_buildings, brownout_hours_today, weather, plants: id, kind, capacity_mw, available_mw, output_mw_est, upkeep, pollution}, transit {routes: id, stops, buses_wanted, buses_running, riders_today, waiting, length_m, broken_legs, runnable; totals}, rail {facilities: id, kind, rail_connected, road_connected, works, waiting_passengers, freight units; trains by kind; riders, freight}, parking {capacity, used, occupancy, parked_on_street_today, last_shortage_at}, traffic {vehicles, cars, trucks, buses, mean_speed_kmh_moving, stopped, stuck, lanes, intersections, signals} |
| `alerts` | the ranked list below |

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
scarce_resources. The thresholds are named constants at the top of `remote/report.rs`: hungry =
satiety < 0.3; starving = 2+ hungry days (the sim sends people away at 4); a food seller is
understaffed below 50 %; unpowered = under 60 % of demand met; rent stress = rent over 50 % of
income; stuck = slower than 0.5 m/s on a trip older than 900 s; high demand ≥ 0.6; unemployment
≥ 15 %; scarce = average price ≥ 1.3 × base.

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
- One of `population`, `land_value`, `jobs`, `hunger`, `vacancy`, `power`, `traffic`: `unit`,
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
running app, not in saves**: they start empty when the game starts and start over on every load
(the `load` command, or the clock going backwards or jumping more than two days at once).

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

`{"cmd": "export", "path": "optional.json"}` writes `report` (all sections), `map_summary`,
`history` and `tables` (every row of every `list` kind) as one JSON document, by default to
`<working dir>/exports/city-<unix>.json` (`/exports` is git-ignored). Reply: `path`, `bytes`,
`took_ms`.

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

- **Localhost only**, no authentication: anything on the machine can drive the game. Do not
  forward the port. Set `ND_REMOTE=0` to turn it off.
- One frame of latency per request; the HTTP thread gives up after 5 s (HTTP 504), e.g. while the
  window is minimised and frames stall.
- Remote edits go through the same `World` calls as the tools, so they behave identically,
  including undo within a project, costs and refusals.
- `load` replaces the city: all ids from before are invalid.
