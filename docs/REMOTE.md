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
- Environment: `ND_REMOTE_PORT` picks the port (default 4817); `ND_REMOTE=0` disables the server. `ND_WINDOW=WxH` sets the starting window size (default 1600x900), and `ND_UI_SCALE` the text size, for layout checks.
  [home] Paths the API takes or returns (`saves/...`, `screenshots/...`, `exports/...`, the flags file) are inside the game's home folder: the exe's folder, or `ND_HOME` (`ND_HOME=.` for the working directory).
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

**Other names.** Some commands answer to other names as well (`help` lists them under `aliases`):
`add_interchange` (`interchange`); `campus` (`university`); `city_name` (`city_identity`); `cleanup` (`clean_up`); `committee_meeting` (`meeting`); `corps` (`corp`, `megacorp`); `dirt_at` (`contamination_at`); `district` (`districts`); `estate` (`land_market`); `favourite` (`favorite`); `graphics` (`quality`); `grounds_junctions` (`park_junctions`); `heat_vision` (`heat`); `level_crossings` (`level_crossing`, `crossings`, `rail_crossings`); `pressure` (`pressures`); `roads` (`road_condition`, `road_budget`); `shore` (`waterfront`); `skyline` (`towers`, `height_limit`); `street_events` (`nights`); `subway` (`metro`); `taxes` (`tax`); `tolls` (`toll`); `tourism` (`tourists`); `tow_car` (`tow`, `boot`); `zoom_to` (`zoom`).

`help` is checked against the dispatcher by a unit test (`remote::dispatch_tests`): every command
it runs is documented in `help` or named as an alias, and nothing else is.

### Overview and time

| Command | Fields | Reply |
| --- | --- | --- |
| `help` | | every command and its fields |
| `status` | | clock, paused, speed, effective_speed, population, buildings, under_construction, lots, homes/jobs, treasury, ledger_yesterday, power, weather, land, market, demand, traffic, transit, rail, render (fps, ticks_per_frame, triangles), camera, tool, selection, project, overlays, lighting, message, tool_status, event_seq, favourites, `windows_open` (the tool windows), `city_data` {open, tab, sub}, `build_window` {open, section}, `title_screen`, `climate` {climate, label, precipitation: null or {kind rain/snow, intensity 0..1}}, `alerts_top` (the first 5 of `report`'s alerts) |
| `memory` | | [memory] `working_set_mb`, `peak_working_set_mb`, `commit_mb` (the process's private commit); `meshes`, `materials`, `images`, `entities`; `render` {building_entities, people_meshes}; `world` {citizens, buildings, cars, lots, day}; `heap` {live_mb, peak_mb, biggest_block_mb} with the `heap-count` cargo feature only (zeros otherwise) |
| `pause` | `paused` bool (omit to toggle) | `paused` |
| `speed` | `index` 0..5 (1×, 4×, 16×, 64×, 256×, 1024×), or `value`: a multiple of real time for what moves, snapped to the nearest step on a log scale. At 1× people walk at their true pace; the clock runs 15× faster than that | `speed_index`, `speed`, `ticks_per_second`, `label` |
| `step` | `ticks` (default 1, max 100000) | `ticks` (run), `clock`, `took_ms`; `stopped` when a committee meeting was called |

`step` runs the ticks immediately, on the main thread, even while paused (the same `World::tick`
the frame loop uses), and stalls the frame for as long as it takes: a young city runs thousands of
ticks a second, a big one far fewer. A step that has started is waited for (up to 5 minutes), but
the frame freezes meanwhile, so prefer several shorter steps. A big step also floods the event
ring (see `events`). The committee's daily round (samples, alerts, the meeting calendar) runs for
every new day the step crosses; when it calls a meeting, the city pauses for it as in game and the
step stops there: `ticks` says how many ran and `stopped` says why (`committee_meeting` runs it).

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
| `load` | `path` (`.ron`, see the file rules), `confirm` (see below) | as the Load button: the selection and the tools' ids reset (their settings stay), the city comes up paused; closes the title screen. A failed load is an error and leaves the running city as it was |
| `list_saves` | | `saves`, newest first: `path`, `name` (as typed for named games; "Latest quick save", "Autosave", "Quick save, day N" otherwise), `kind` (named / latest / autosave / dated), `modified_unix`, `bytes`, `day`, `population`, `treasury`, `buildings`, `landscape` (null for saves without a sidecar), `thumbnail` (absolute PNG path, or null), `city` (the city's name; an old named save's save name; "Unnamed city" otherwise), `game_id` (null before ids); `by_city`: `[{city, saves: [path, ...]}]`, the grouping the title screen and the Saves window show |
| `city_name` | `name` optional | without `name`: `name` (as kept, "" when unnamed), `display_name`, `game_id` (null for a save from before ids until it is next saved), `history_entries`. With `name`: renames the city (control characters dropped, spaces folded, at most 40 characters; "" leaves it unnamed); saves from then on carry it |
| `game_history` | `action` list (default) / bookmark / unmark / mark / revert, and per action below | the running game's history of autosaves (below) |

The **title screen** comes up at startup (unless `ND_TITLE=0`) over a paused city: [title] the
games as cards with their pictures on the left (grouped by game id, the game played last first and
largest), the picked game's saves on the right (quick saves, autosave, dated quick saves and its
history autosaves, newest first, with day, population and age) to pick and Load; Continue, New
game, Play this map. [identity] Loading an example city at its first day (no people, no game id
of its own or a starting town's name, like the bundled Perfect AI City) starts a new game: a new
id and a suggested name. `load`, `new_game`,
`generate_city` and `seed_demo` close it; `window {name: "title", open: false}` closes it and runs
the city (as Play this map), `open: true` brings it back. `status.title_screen` says whether it is up. Scripts that
screenshot the city should close it first.

Paths are limited to `saves/`, `screenshots/`, `exports/` and the temp folder (see
[Who may call](#who-may-call)). Relative ones are taken from the game's home folder ([home]: the
exe's folder, or `ND_HOME`).

**City name and game id.** Every world carries its city's name and a game id (saved with it,
and repeated in each sidecar as `city` and `game_id`). `new_game` and `generate_city` make a
fresh id and take an optional `name`; the New game window and the title screen ask for one.
`status.identity` is `{name, display_name, game_id}`. A save from before ids loads with no id
and gets one the next time it is saved (any save, or the next autosave). Its city name is the
name it was saved under when it is a named save (the sidecar's name as typed, else the file
name with spaces), and "Unnamed city" for the quick save slots and the autosave.

**Game history.** Each daily autosave is also written to
`saves/history/<game id>/<unix ms>-day<N>.ron` with its sidecar and picture; the newest 8
unmarked entries of a game are kept, then 8 older ones 5 days apart, marked ones always.
[saves] Every save the game writes (named, quick, autosave, history, the API's `save`) is
gzip-compressed RON (it starts with the bytes `1f 8b`); `load` takes that or plain RON, so
older saves load. Probes read either with `nd_sim::World::read_save`. `game_history`:

- `{}` or `{action: "list"}`, optional `game_id` (another game), `marked_only`: `entries` newest
  first, each `path`, `file`, `day`, `population`, `treasury`, `buildings`, `saved_unix`, `city`,
  `bookmark` (null when unmarked), `thumbnail`.
- `{action: "bookmark", path | file, label}` names a moment (`unmark` or an empty label clears it).
- `{action: "mark", label}` saves the city now into its history with that name.
- `{action: "revert", path | file, confirm: true}` loads that entry as `load` does (tools reset,
  paused). Without `confirm: true` it refuses and says what would be replaced. Newer entries stay
  listed, so a revert can be undone the same way. Only entries of the running game are accepted.

`window {name: "history"}` opens the Game history window (the Saves window's "Game history"
button): the same list with pictures, Mark / Rename / Unmark, Revert with a confirmation, and
"Mark this moment".

```json
{"cmd": "city_name", "name": "Cobalt Reach"}
{"cmd": "game_history"}
{"cmd": "game_history", "action": "bookmark", "file": "001759800000000-day12.ron", "label": "first metro line"}
{"cmd": "game_history", "action": "revert", "file": "001759800000000-day12.ron", "confirm": true}
```

Every save is written to a temporary file and renamed over the target, so a crash mid-write
leaves the old file whole. The daily autosave (`saves/autosave.ron`) copies the world on the
main thread, then serialises and writes it on a worker thread.

#### Replacing the city: `confirm`

`load`, `new_game` and `generate_city` throw the running city away. When it has play time that no
save holds (its clock has moved since it was last saved, loaded or started; the daily autosave does
not count, the next day overwrites it), they are refused unless the request carries
`"confirm": true`, and the error says what would be lost:

```json
{"ok": false, "error": "new_game replaces the running city (day 3, 14:20), which has play time no save holds; save it first, or send confirm: true to replace it anyway"}
{"cmd": "new_game", "preset": "coast", "confirm": true}
```

A city just started, saved or loaded needs no `confirm`, and neither does an untouched one (no
building, no project and no road beyond those it started with, such as the empty landscape a
`ND_TITLE=0` instance starts on). `game_history {action: "revert"}` always needs it. Edits made
while paused do not count as play time.

### Landscape and new games

| Command | Fields | Reply |
| --- | --- | --- |
| `new_game` | `preset` river_valley (default) / islands / coast / delta / plains / flat, `seed`, `water` 0..1, `relief` 0..1 (height variation), `branching` 0..1, `islands` 0..8, `climate` temperate / desert (default) / tropical / arctic / volcanic (also `iceland`) (`highway` is ignored: every new game has the highway and the trunk line); omitted levers take the preset's; optional `name` (the city's name); `confirm` (see above) | throws the city away and starts on the landscape with a new game id; the selection and the tools' ids reset. Reply as `landscape` plus `identity` |
| `generate_city` | `preset` grid_plaza (default) / coastal_river / old_town / twin_centres / perfect_city, `seed` (default 1, perfect_city 5; seeds the landscape and the city), optional `landscape` (a `new_game` preset to lay the city on instead of the preset's own) and `water`, `relief`, `branching` levers, optional `climate` (as `new_game`; default desert), optional `name`, `confirm` (see above) | throws the city away and starts a new game with a whole generated city on it (below); the selection and the tools' ids reset. Reply as `landscape` plus `city`: `preset`, `seed`, `arterial`/`collector`/`local` (`roads`, `length_m`), `zones`, `lots`, `interchanges`, `way_in`, `centres`, `took_ms`, `furnished` (perfect_city: one line per placement it made, refusals marked REFUSED) |
| `landscape` | | preset, `climate` and levers, `rivers` (kind river/stream, width, current_mps, hydro_ok, source, mouth, length), `lakes`, `sea`, `hot_spots`, `highway` (entries, exits, through_cars), `bridges` (count, metres, on the highway), `ports`, `ships`, `port_stats` |
| `terrain_at` | `x`, `y` | `natural_height`, `ground_height` (graded under roads and lots), `surface_height` (bridge decks included), `slope`, `water` (kind, current, width, flow), `nearest_shore`, `hot_spot`, `lot`, `power_sites` (hydro/tidal/geothermal ok or why not, for the lot there), `docks` (whether a shipping port or ferry terminal would go there, or why not), `survey` (every land survey layer's value 0..1 there), `access` (`park_min`, `nightlife_min`: minutes on foot along the walk network to the nearest park / open nightlife, null when none can be walked to; `value_for_homes`: what that adds to a home's land value; `text`; the whole object is null off the walk network) |
| `resource` | `x`, `y` (or `z` for `y`) | the wind and sun for power plants at a point: `climate`, `water`, `wind {site_factor` (the map's wind field there against open level ground, 0.35..1.8: windy on hills, ridges, high ground and by wide water, sheltered in valleys), `mean_ms` (ordinary-day wind there), `now_ms`, `typical_yield` (a wind farm's share of nameplate on an ordinary day), `yield_now`, `cut_out_now}` (the wind there is past the turbines' cut-out), `solar {site_factor, climate_sun, sun` (0..1.1, 1 = the desert's open ground), `yield_now}`. `terrain_at` carries the same under `resource` |
| `weather` | | `label`, `cloud` 0..1, `wind` (the 0..1 drift), `wind_ms` (over open ground), `temp_c`, `spell` (normal / heat wave / cold snap / storm), `spell_hours_left`, `climate`, `turbine {cut_in_ms, rated_ms, cut_out_ms}` |
| `grid` | | the time-of-use grid: `hour`, `import_price` and `export_price` now (per MWh), `mean_import_price`, `demand_mw` now, `full_draw_mw` (every building flat out, the old figure), `demand_factor_now`, `weather`, `day_ahead` (24 rows from this hour: `hour`, `temp_c`, `demand_mw`, `import_price`, `export_price`, if the weather holds), `last_24h` (recorded `day`, `hour`, `demand_mw`, `supply_mw`, `imported_mw`, `exported_mw`, `import_price`, `temp_c`) |
| `grant` | `amount` (default 100000) | dev: adds money to the treasury |
| `set_weather` | `cloud` 0..1, `wind` 0.05..1, `temp_c` | dev: sets the sky now (it drifts back toward the climate's hour by hour); reply `cloud`, `wind`, `temp_c`, `spell`, `label`, `climate`, `weather_climate`, `precipitation` |

```json
{"cmd": "new_game", "preset": "delta", "seed": 12, "water": 0.7, "branching": 0.9}
{"cmd": "new_game", "preset": "coast", "seed": 4, "climate": "arctic"}
{"cmd": "terrain_at", "x": -420, "y": 100}
{"cmd": "resource", "x": 300, "z": -650}
{"cmd": "grid"}
```

**Ferry routes.** [ferry-routes] `landscape.ships` rows carry `id`. `select {ship: id}` selects a
ferry or freighter (a click on its hull with the Inspect tool does too; `camera {follow}` and the
inspector's Follow work on it); `inspect {ship: id}` (or `inspect` with one selected) gives `id`,
`kind`, `title`, `doing`, `at`, `passengers`, `cargo`, `home` (its terminal) and `route` {from, to,
colour}. `inspect {building}` of a ferry terminal or shipping port adds `routes`: one row per route
calling there, `kind` (ferry / freight), `to` (the far end's name), `end` ({kind: terminal, building}
/ {kind: town_off_map, index, edge} / {kind: open_sea}), `end_at` (its berth, or where the route
leaves sight), `running`, `colour`, `ships` [{id, doing, passengers}], `headway_min` (both ends'
boats together, on the rotation as it stands), `next_departure` (the next boat out from this dock on
it, as `tick_time`), `passengers_today` (both ways) and `length_m`. With a dock or ship selected the
map draws its routes bright with direction chevrons and the city's other routes dim.

```json
{"cmd": "select", "ship": "3:1"}
{"cmd": "inspect", "building": "120:4"}
```

[w1] `inspect {building}` of a building site or demolition has `site` {kind (going_up,
coming_down_rebuild, coming_down), progress, coming_down_floors, next: {building, name, land_use,
tier, floors, height_m, joins} or null}: what will stand on the lot when the work is done (the
site itself, a redevelopment's successor, a campus annex, or the building next door that bought
the plot, `joins: true`).

**Climates** (`nd_sim::Climate` on the landscape's levers; old saves are Desert). Each adds to
the generated map from its own random stream, so the river, lakes and hot spots the preset makes
stay where they are: Desert is the original generator (the driest); Temperate adds a stream or
two and a lake; Tropical more streams, lakes and three more offshore islands; Arctic two more
lakes (drawn frozen, with ice along the sea's edge: still water to every rule); Volcanic a field
of four to six more geothermal hot spots. The look comes from one table
(`nd_procgen::climate::look`): ground, shore, water, ice, plants (scrub, leafy trees, palms,
conifers, moss and lava rock, in streets and parks too), snow on roofs, steam over hot spots, and
rain (Temperate, Tropical) or snow (Arctic, Volcanic) round the camera when `cloud` is over 0.6
(not at the Low graphics tier).

#### Weather, demand and prices [grid-weather]

- **Weather** follows the map's climate (`landscape.climate`): Temperate mild and mixed, Desert clear
  and hot, Tropical hot with afternoon cloud, Arctic cold, windy and dim, Volcanic cold and windy.
  Once a sim hour cloud and wind drift toward the climate's sky at that hour, the temperature follows
  the climate's mean and daily swing (coolest about 05:00, warmest about 15:00), and a *spell* may set
  in: a heat wave (+8 °C for 2-4 days, clear skies), a cold snap (-9 °C), a storm (half a day to a
  day: low cloud, a gale). Spells come from hashes of the seed and the hour, not the random stream.
- **Demand** varies by hour and land use: homes have a morning bump and the evening peak (about
  19:00), shops and offices run in the day, industry is flat with a night dip, nightlife peaks late,
  parks light up at night; air conditioning adds load above 24 °C and heating below 15 °C.
  `status.power.demand_mw` is this hour's; `inspect building` has `demand_mw` (flat out) and
  `demand_now_mw`.
- **Wind farms** run a power curve on the wind at their lot (the climate's wind times the site
  factor): nothing below 3 m/s, nameplate from 11 m/s, braked above 25 m/s, so in a storm exposed farms
  stop while sheltered ones run flat out. **Solar** follows the sun's height, the cloud, the
  climate's sun and the site, and loses 0.4% per °C above 25.
- **Prices**: the regional grid charges with the square of its own load (the same daily curve and
  weather): cheapest in the small hours, dearest in the evening peak and on heat-wave evenings, a
  mild day averaging the old 2.5/MWh. Exports earn a fifth of the import price at the hour.
- The land survey has `wind` and `solar` layers (`overlay {land: "wind"}`).

`window {name: "new_game"}` opens the New Game window, [wizard] step by step: 1 landscape (preset
list, climate, levers, mini-map), 2 starting town (an empty map or one of the generated cities,
drawn on the mini-map), 3 the city's name (required), 4 review and Start.

**Generated cities** (`nd_procgen::citygen`, after Chen et al. 2008, "Interactive Procedural
Street Modeling", the method ProbableTrain's MapGenerator uses). Streets are traced through a
tensor field blended from grids, radial centres and the lines of the land (rivers, the coast, the
highway, the railway): arterials about 330 m apart, collectors about 165 m, local streets 64 x
108 m, a waterfront street along each river bank and the coast. Dangling ends are joined to the
street ahead, and the network is cleaned to the game's rules (junctions at least 22 m apart and
32 degrees, no sliver blocks, one network) before it is laid as road gestures (arterials 2+2
lanes, collectors and locals 1+1; curves sampled every 7 m, straight into every junction). The
blocks are zoned by a land-use model: shops round the centres and along the arterials, works by
the railway, the highway and the edge of town, parks on the waterfronts and plazas, homes
between. Up to two interchanges join the town to the highway, and a coal plant goes on the
biggest works lot. The city stays south of the highway (up to y 340) inside the buildable square.

| Preset | Landscape | Plan |
| --- | --- | --- |
| `grid_plaza` | plains | one grid round a radial plaza |
| `coastal_river` | coast, sea close in | streets follow the shore and the river; the port centre at the river mouth |
| `old_town` | river valley | a twisting radial old town, modern grids at other angles either side |
| `twin_centres` | plains | two radial centres, each with its own grid, grown together |
| `perfect_city` | temperate coast (seed 5) | [perfect-city] the "Perfect AI City": a planned avenue grid (collectors 2+2 too), three diamond interchanges, downtown by the sea, works downwind (east, across the river), farms upwind, and every service, plant, transit line, district and a 50 million treasury laid down at day 1 (`citygen::furnish`, DESIGN.md) |

```json
{"cmd": "generate_city", "preset": "old_town", "seed": 3}
```

### Planning: roads, zones, parcels, projects

Like the tools, new gestures go into the active **project** and change nothing until it is
implemented. Pass `"implement": true` to implement straight away, or send `project` later.
Several named projects can be pending; the active one is edited, the others are previewed dim.
Parcels exist along every road before anything is zoned; zoning (a polygon, or `paint`) gives
them a land use.

| Command | Fields |
| --- | --- |
| `plan` | none: every gesture in the effective plan (id, kind, points, lanes, land_use), `pending_gesture_ids`, project steps |
| `add_road` | `points` `[[x,y],...]` or `[[x,y,level],...]` (2+; level metres above the ground, negative below), `elevation` (a level for every point without its own, or a list), `tunnel` (default level -8 m), `kind` street (default) / path / rail, `lanes_fwd`, `lanes_bwd` (0..4, default 1/1, streets only), `curved`, `snap` (default true: points snap to a road at about their level; an end snaps to any road in reach, the one at about its level first, and takes that road's height there whatever level it was given (as the network lays it; [r4-ramp]); a path end also snaps onto a park or grounds gate nearer than any road), `from_junction` / `to_junction` (paths only: start or end on that `grounds_junctions` index), `use` ([w1-roads] `mixed` or `car_only` for a street: what it carries, stored on the road; omitted, it follows the lanes as before, a one-way street of 3+ lanes being a highway), `implement`. Reply adds `joined` (paths: the gates its ends are on, see `grounds_junctions`) and `grade_warnings` (too steep, crossing at an awkward height). Refused (400) if it would meet the railway at a level neither at grade nor clear of it, e.g. a portal ramp part way down: the error says how far to lower or raise it |
| `road_use` (also `set_road`, `road_edit`) | [w1-roads] the Edit tool's upgrade panel for a road: `gesture`, `use` (`pedestrian` / `mixed` / `car_only`: Mixed has pavements, street speed and lots along it; Car-only no walkers, no pavements, highway speed, fronts nothing; the lanes stay), `lanes_fwd`, `lanes_bwd` (the use stays), `implement`. Reply: gesture, use, was, lanes_fwd, lanes_bwd, implement_refused. The `tool` command takes `use` too (the Road tool's Use dial), and its reply and a road `gesture` carry `use` (and `use_set`: stored on the road, or read from its lanes for a road from before) |
| `add_roundabout` | `center` `[x,y]` (or `pos` / `x`,`y`; snaps onto a junction within 12 m or a street within 6 m), `radius` 12..120 m to the ring centreline or `diameter` 24..240 (default the tool's), `lanes` 1 / 2 / 3, `implement`. Reply adds `roundabout` {`center`, `radius`, `diameter`, `lanes`, `joins`, `cuts_roads` (gesture ids), `takes_buildings`}; refused where it would partly overlap another roundabout (nesting one wholly inside another, with a lane and pavement between, is fine) |
| `add_interchange` (alias `interchange`) | `from`, `to` `[x,y]`: the cross street's ends, one each side of the highway; `lanes_fwd`, `lanes_bwd` (default 2, 2), `check` (only plan it), `implement` | [interchange] a diamond interchange as the Road tool's Interchange preset lays it: the street under the highway (kept when one runs along the line already) and four ramps meeting it at a signalled junction each side. Reply adds `gesture_ids`, `interchange` {`kept_street`, `terminals`, `crossings`, `roads` [{points, lanes_fwd, lanes_bwd}]}, or `refused` {`why`, `at`} (the spot). `tool {preset: "Interchange"}` picks the preset; with `draw` [[a],[b]] its hologram and `interchange_refusal` show. Dev: `rebuild_roads` lays the roads again from the plan with the current rules |
| `add_zone` | `polygon` `[[x,y],...]` (3+), `land_use` residential / commercial / industrial / civic / park, or leisure ([r4-leisure] commercial land of the leisure sub-zone; `entertainment`, the old zone, is an alias), `sub_zone` (commercial only: auto / neighbourhood / retail / offices / leisure), `implement`. Lot rows (`find`, `list lots`, `inspect` a lot) carry `sub_zone` (commercial land's: its key or auto) and `zoned_as` (leisure land is Commercial); a lot's detail also has `sub_zone_auto_pick` (what the city would build there now) |
| `bulldoze` | `gesture_id` (the whole gesture), `implement`; or `at` `[x,y]` ([pt-ux] as a Bulldoze click there: only the road's piece between its junctions, or to its end, goes; the rest of the road stays, keeping its name; one undo step; reply adds `piece` {`gesture_id`, `from_m`, `to_m`, `roads_left`}; the Bulldoze tool previews that piece in red); or `building` (id): a city building (plant, battery, service, attraction, depot, metro station, cargo hub, city parking) goes at once and its lot goes back to the zoning plan (or unzoned ground). Grown buildings, stations, yards and docks refuse |
| `paint` | `land_use` (or `none` to unzone; `beach` paints a public beach, see the waterfront section), then `at` `[x,y]` + `radius` (the brush), `rect` `[[x0,y0],[x1,y1]]` (the marquee) or `polygon`; `industry` (industrial only: auto / farming / oil / refinery / manufacturing / logistics / fishing / tech), `sub_zone` ([r4-leisure] commercial only: auto, the city picks per lot from the street, homes near, land value, demand and the district's story / neighbourhood / retail / offices / leisure; `land_use: leisure` or `entertainment` is commercial with the leisure sub-zone), `implement`. Reply: `painted`, `touched`, `refused` (`[{parcel, center, why}]`), `gesture_id`; `no_parcel` (why, when `at` touched no parcel: water, too far from a road, past the buildable land) |
| `project` | `action`: list / new / switch / rename / delete / park / start / implement / undo / redo / discard; `id` (default the active project) or `name` to pick one; `name` for new and rename. Reply: `projects` (`id`, `name`, `active`, `steps`, `redo`, `conflicts` `[{gesture, project}]`, `shared_parcels`), `warnings` from the last implement |
| `grounds_junctions` (or `park_junctions`) | optional `lot`, `kind` (park / plaza / stadium / campus / station / beach / attraction), `near` `[x,y]` + `radius` (default 150 m). [park-paths] The gates on the edges of parks and other public grounds (a stadium, a monument's plaza, a school's campus, a station) where a path may join their walkways: the Path tool's snap rings. Reply `junctions` `[{index, pos, kind, label, lot, attached, walk_node, gesture_id}]` (`attached`: a path is joined there, `walk_node` the grounds' walk node it joins, `gesture_id` the path) and `links` (every joined path end). A path whose end is on a gate when its project is implemented is joined: the park lays a walkway out to meet it and walkers route through. When the grounds go (rezoned, bulldozed, re-cut away) the path is cut back to its last point before that end, or removed if fewer than two points are left; the event log gets `PathTrimmed` {`gesture_id`, `removed`, `grounds`} and the game a toast |
| `find` `{what: "parcels"}` | per parcel: `index`, `center`, `area`, `land_use` (built), `planned`, `landlocked`, `street`, `rough`, `accepts`, `lot`; filter `land_use`, `zoned`, `landlocked` |

```json
{"cmd": "add_road", "points": [[300, -200], [300, 200]], "lanes_fwd": 2, "lanes_bwd": 2}
{"cmd": "add_zone", "polygon": [[310, -60], [400, -60], [400, 60], [310, 60]], "land_use": "residential"}
{"cmd": "project", "action": "implement"}
{"cmd": "project", "action": "new", "name": "Harbour"}
{"cmd": "paint", "land_use": "industrial", "industry": "logistics", "rect": [[300, 10], [420, 60]]}
{"cmd": "project", "action": "switch", "id": 1}
{"cmd": "grounds_junctions", "kind": "park", "near": [120, 40]}
{"cmd": "add_road", "kind": "path", "points": [[150, 220], [0, 0]], "to_junction": 3, "implement": true}
```

Reply: `gesture_id`, `implemented`, `project` (`id`, `name`, `steps`, `redo`), `projects`, `plan_version`.

Road works never take the city's own structures (depots, stations, yards, plants, service buildings, docks, metro stations, cargo hubs). An implement whose roads would run over one, or leave one with no street, is refused and the project stays pending: `implemented` is false (`project implement` replies `did: false`), `implement_refused` gives the reason, and every project reply lists `blockers` (`[{id, name}]`), the structures the pending roads run over now (drawn with a wide red border and a red pin in the game; `plan` gives the full check, with the structures the rebuild would lose and the road that takes each). Move the road or bulldoze the structure, then implement again.

### Placing buildings

`place {kind, x, y}` (or `pos: [x, y]`):

[free-placement] Everything that goes on a lot goes on any lot, whatever its zone, or on
unzoned ground with road access at the point: no zoning first (`World::place_building`,
`World::placement_site`). On unzoned ground a lot is made for it from the parcel there, or
the parcel joined with its unzoned neighbours when one is too small; [big-sites] on a lot too
small for it (a stadium wants 2,400 m²) the neighbouring lots it needs are joined to it (what
grew there is bought out; the check reply's note says how many plots). Only physical rules
refuse: no road access, a street out front where it needs one, the lot's size, the site
(river, coast, hot spot), a station's ground, another city building ("bulldoze it first"),
park land (but for a metro station), the treasury. The ground under the new building leaves its
zone: the lot's and the building's `land_use` read `Civic` (building rows add `use`, the
building's own use for power, wages and venues: industrial for plants and depots,
entertainment for a stadium, ...) and lot rows carry `city_ground: true`, zone tallies leave it out, `paint` refuses it while the building stands, and `bulldoze
{building}` gives it back to the plan. The reply's `message` ends with what happened to the
zoning ("unzoned parcel: fine, a lot is made for it", "takes this lot out of the residential
zone"). With `check: true` nothing is built: the reply is `{ok, note, lot, parcels,
leaves_zone, outline}` or `{ok: false, reason}`. Off any lot or parcel the nearest vacant lot
within 80 m is used, as before.


- `station`, `freight_yard`, `central_station` (also `central`, `main_station`): rail pieces, placed
  anywhere (`World::place_rail_facility`); no road is needed. The reply message says whether track and street are
  joined. [dock-access] A passenger station on the track whose street is not yet on the road
  network is a halt: trains call there, nobody gets on or off, it hires nobody. Draw a road to
  a marked end of its street stub (`road_access.stub_ends`) and it opens. The Central Station (one per city) is also a metro station and a bus terminal (a depot).
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
  under the point (or unzoned ground there), else the nearest vacant lot within 80 m (`build_depot`, `build_parking`,
  `build_power`). Hydro needs a lot within 30 m of a river or stream running at 0.8 m/s or more,
  tidal a lot on the sea shore, geothermal a lot over a hot spot (`terrain_at` says which apply).
  Solar and wind are sized to the lot: nameplate, cost and upkeep scale with its area (600 / 800 m²
  for the quoted figures, capped at 6× on one lot); the reply's message gives the MW, what was paid
  and the Sunfield or Wind Park it joined. A lot below the kind's minimum may still take one when it
  joins plants of its kind next door and the joined lots reach the minimum.
- `plant_preview {kind, x, y}` (read-only): what a plant would be on the site a click there takes
  (a lot, or unzoned parcels joined into one: `lot` null, `unzoned_parcels`, `outline`) before
  placing it: `area_m2`, `size_scale`, `capacity_mw` (its own, group bonus included), `site`, `cost`,
  `upkeep_per_day`, `min_area`, `max_area`, `joins` (the Sunfield / Wind Park it would join, or null),
  `joined_lots`, `joined_area_m2`, `joined_capacity_mw`, `adds_mw` (what placing it adds to the
  joined installation), `small_lot_joins` (allowed only by joining), `ok`, `problem`. The same
  figures are in the Buildings tool's hover `note` (`tool {hover}`).
- `shipping_port` (or `port`), `ferry_terminal` (or `ferry`): at the water's edge within 90 m of the
  point (`World::place_port`). Ports need a river at least 40 m wide or the sea; ferries any water
  16 m or wider. Each builds a pier. [dock-access] No road is needed: unless a street is already
  along its landward side, a dock lays its own access road, a 1+1 street with pavements running
  14-32 m inland from its gate (turned or shortened round water, buildings and other docks), and
  the far end (`road_access.stub_ends`) is where a road is drawn onto it; it snaps there like any
  road end. Until it is on the road network (a road component with zoned lots, or one reaching
  off the map) a ferry terminal's boats still sail but carry no one, a port lands nothing and a
  marina or fishing port does not open; nobody is hired and no lorry or taxi is sent there.
  Docks placed over zoned land take the ground out of the zone (zoned lots under the footprint
  go) and are drawn as themselves at once; `bulldoze {building}` on a dock takes it and its pier
  away and gives the ground back to the plan (its access road stays).
- `toll_booth` (also `toll`), `toll_plaza` (also `plaza`): [toll-fit] the Build window offers one
  toll plaza, `toll_booth`, a kiosk per lane of its road and priced by it (1,500 + 1,250 a lane,
  upkeep 15 + 22.5 a lane a day; widening the road under it charges the new kiosks); the old
  six-booth `toll_plaza` stays for the API and old saves. Across the road at the point
  (`World::place_toll`), not on a lot: the nearest street or highway piece within 15 m that has a
  straight run either side (booth 18 m, plaza 50 m between junctions or bends; a click near a
  junction slides along to fit), at least 40 m from another booth. Cost 4,000 / 15,000, upkeep 60 /
  200 a day. Reply: `toll` (the booth id, an integer), `message`, `row` (as in `tolls`), `treasury`.
- `vertiport` (also `skyport`, `drone_pad`) with `tier` low / mid / high: the air taxi base (`nd_sim::air`), on any
  lot but parks; works once the old building on the lot is cleared.
- Services and attractions: `police`, `fire`, `hospital` (`clinic` is a low-tier hospital), `school`, `jail`,
  `dispatch`, `monument`, `stadium`, with `tier` low / mid / high (default mid) or a suffix
  (`"fire:high"`), on the lot or unzoned ground under the point (`World::place_building`, `build_service`). Whatever grew
  on the lot is bought out and pulled down; another civic building refuses ("bulldoze it first").
  Refusals name the reason: park land, lot area, no street out front (police, fire, health, stadium),
  treasury. Costs, upkeep, jobs, coverage radius, vehicles and capacity per tier are in
  `nd_sim::services::ServiceKind`.
- [attractions] Landmark attractions, one of each per city, one size (any `tier` is ignored):
  `art_museum`, `science_museum` (natural history and science), `history_museum` (city history and
  design; also `museum`), `pier` (pleasure pier), `grand_park`, `zoo`, `observatory`, `aquarium`,
  `botanical_garden` (also `conservatory`), `concert_hall` (also `opera_house`). Each needs a site of
  several plots (art 2,200 m², science 2,600, history 1,800, pier 1,200 ashore, grand park 7,000, zoo 5,200,
  observatory 1,500, aquarium 2,400, botanical 3,800, concert hall 2,600; `ServiceKind::min_area`):
  the lot under the point takes in its neighbours (up to 48 plots, notches allowed), whatever grew on
  them bought out, as for a stadium; on unzoned ground the parcels there are joined. Refusals name
  the reason: `one per city: <name> is the city's <kind> already`; the pier `a pier goes on the sea
  shore...` (the site must come within 14 m of the sea, not a lake or river, with open sea for its
  deck 60-120 m out); the observatory `an observatory needs high ground: this site is +N m from the
  middle of town (M m up), it needs +8 m or more` (`World::rise_above_town`: against the median
  ground height of every lot; a flat map has none); a long thin run of plots `the plots here only join
  into a long thin strip...`. `inspect {building}` adds `attraction` {kind, label, about,
  open_hours, open_now, capacity, admission, visits_today/yesterday (townsfolk), tourists_today/
  yesterday, takings_today/yesterday, visits_total, tourist_pull, land_value_lift {radius_m,
  raw_weight}, pier {root, tip, length_m, width_m, head_m} (the deck over the water), rise_above_town_m}.
  The grand park, zoo, botanical garden and pier lay out walkways and benches (`park` in `inspect`;
  their gates are `grounds_junctions` of kind `attraction`), the pier's out along its deck.

Reply: `building` id, `message`, `row`, `treasury`; or the sim's reason as `error`. For a dock,
station or yard the reply adds [dock-access] `road_access` {`connected`, `stub_ends` ([[x, y]]: the
free ends of its street stub to draw a road to; empty once joined), `problem` (what is held up,
or null)}; `inspect {building}` carries the same `road_access`, and for a dock `dock` {kind,
road_connected, reaches_elsewhere, works, ferries_sail, berth, stub}. `report`'s alerts add
`dock_not_connected`.

```json
{"cmd": "place", "kind": "solar", "x": -210, "y": -210}
{"cmd": "place", "kind": "fire", "tier": "high", "x": 120, "y": 40}
{"cmd": "place", "kind": "pier", "x": 40, "y": -610}
{"cmd": "place", "kind": "dispatch:low", "x": -80, "y": 40}
```

#### What is already on the map: `build_highlight` [build-highlight]

Picking a Build window section that puts down buildings (Power, Services and its sub-sections
Safety / Health and education / Sanitation, Transit's Buses / Metro / Air / Parking / Freight,
Rail's stations, Water, Attractions; not roads, zones, districts, lines or tolls) highlights every
building of that section on the map: a coloured outline and light wall on its lot and a marker
over it in its kind's colour (police blue, fire orange, health green, schools yellow...). A picked
item (a tile clicked: Hospital) shows its kind strongly and the rest of the section faintly. With
them the service areas the sim uses: police, fire, health and schools tint the lots each station
reaches **by road** (`World::service_reach`, one station's share of the coverage), additively, so
overlap is brighter; trash depots, parking wardens, metro stations (and the Central Station),
vertiports and cargo hubs show their straight-line reach as a ring (lots inside tinted, but for
the hub's city-wide one); bus depots show every stop's 110 m walk-in ring. When one home-serving
kind is shown, homes it leaves out are red. Power plants carry their output (`out / built MW`)
instead of an area: the grid is one network. The Build window's info column says it in a line
("2 hospitals · 84% of homes covered by road · 3 areas uncovered"), with a colour key and the
uncovered places as buttons that fly the camera there. It is worked out again only when the
roads, the matching buildings, homes, districts or bus stops change (checked twice a second).
It goes when the section changes to one without buildings or the window closes.

`build_highlight {category}` opens the Build window there: a section key (`power`, `services`,
`transit`, `rail`, `water`, `attractions`), a sub-section name (`safety`, `health and education`,
`sanitation`, `parking`, `metro`, `buses`...) or a kind (`health`/`hospital`, `police`, `fire`,
`school`, `trash`, `metro_station`, `coal`...; a kind also picks its tile, so the Buildings tool
is in hand). `build_highlight {off: true}` closes the window. With no fields it reports what is
shown now: `{on, section, sub, focus, buildings: [{building, name, kind, level, label}], summary, detail, homes_covered, uncovered:
[{name, at, homes}]}` (worked out the frame after a change, so read it after a `step` or a
moment).

```json
{"cmd": "build_highlight", "category": "health"}
{"cmd": "build_highlight", "category": "safety"}
{"cmd": "build_highlight"}
{"cmd": "build_highlight", "off": true}
```

### Taxes and tolls

`taxes {}` reads the tax schedule; the same reply comes back from every setting form below.

| Field | Meaning |
| --- | --- |
| `set` | `{line: rate, ...}`: `line` is a key (`residential_low`, `residential_mid`, `residential_high`, `grocery`, `bakery`, `gadget_shop`, `parking`, `nightlife_low`, `nightlife_mid`, `nightlife_high`, `farming`, `oil`, `refinery`, `manufacturing`, `logistics`, `fishing`, `tech`; industry kinds also by their `industry` names) or a land use (`residential`, `commercial`, `leisure` (the nightlife lines; `entertainment` also works), `industrial`) to set its whole group. A rate is a share (`0.2`) or a percentage (`20`, `"20%"`) |
| `category` + `rate` | one line |
| `import_duty` | share added to the price of imports, 0..1 |
| `tourist_tax` | share of a hotel night's price paid to the city, 0..0.4 (standard 0.12); dearer keeps visitors away, cheaper draws more |
| `reset` | `true`: every rate back to the standard, no duty, the standard tourist tax |

Reply: `lines` (per line: `key`, `label`, `land_use`, `rate`, `default`, `max` (0.40 income, 0.30
business), `pull` (1 at the standard rate; below 1 the line is taxed over it and grows less),
`buildings`, `homes`/`jobs`, `residents`/`workers`, `today` (the last midnight's collection),
`yesterday`, `recent_per_day` (running average)), `by_land_use` {buildings, today, yesterday, pull},
`import_duty`, `duty_today`, `duty_yesterday`, `tourist_tax`, `tourist_tax_max`, `tourist_pull` (visits
against the standard rate), `tourist_tax_today`, `tourist_tax_yesterday`, `standard`, `tax_today`,
`changed`.

Taxes are collected at midnight: incomes at the rate of the home's tier (12% standard), business
takings at the rate of the business's line (5% standard; offices and the city's own car parks stay at
5%). The standard rates change nothing; only the difference reaches purses (a household taxed at 20%
pays the extra 8% of its income each day, a farm at 0% keeps its 5%). See `nd_sim::tax` for what
each rate pulls on. The same schedule is in `report {sections: ["economy"]}` under `taxes`.

```json
{"cmd": "taxes", "set": {"oil": 0.2, "farming": 0, "residential_high": 25}}
{"cmd": "taxes", "import_duty": 0.3}
{"cmd": "taxes", "tourist_tax": 0.2}
{"cmd": "taxes", "reset": true}
```

`tolls {}` lists the booths: `booths` (id, name, size `toll_booth`/`toll_plaza`, pos, price,
truck_price, lanes, highway, segment, cars/revenue today, yesterday and total, upkeep_per_day),
`paid_today`, `free_today` (buses, emergency vehicles), `trucks_today`, `revenue_today`,
`upkeep_per_day`, the ledger's `road tolls` today and yesterday. `{action: "price", id, price}` sets a
booth's toll (0..50 per car; lorries pay double), `{action: "remove", id}` takes it away (no refund),
`{action: "inspect", id}` returns the booth in full (as `inspect {toll}`, below); `at: [x, y]` picks the booth within 30 m instead of `id`.
Booths go with their road when it is bulldozed. The Bulldoze tool removes a booth clicked within 8 m.

```json
{"cmd": "place", "kind": "toll_plaza", "x": 0, "y": 420}
{"cmd": "tolls", "action": "price", "id": 1, "price": 4}
```

**Inspecting a booth.** [toll-inspect] `select {toll: id}` (the booth id, an integer; or a click on
the booth with the Inspect tool, `select {at}`) selects a booth: the inspector shows its card and the
map outlines it in gold. `inspect {toll: id}` (or `inspect` / `selected` with a booth selected; also
`tolls {action: "inspect"}`) gives the `tolls` row plus: `kind` ("toll"), `label`, `road` (the road
in words: "two-way street, 2 + 2 lanes", "highway carriageway"...), `charges` ([{heading, lanes}]: the
directions it charges and their lanes), `trucks_*`, `free_*` (buses, emergency and city vehicles let
through) and `avoided_*` (drivers who set off on a way round that they would not have taken with no
tolls: counted when a paying vehicle sets off and a booth near its trip is on the toll-free way but
not on its route; one extra route search only for those trips, no randomness) for today, yesterday
and total; `cars_*` count every vehicle through (lorries and free ones too); `net_per_day`
(yesterday's takings, or today's so far on the first day, less upkeep); `queue` {cars (within 150 m
before the line and under 3 m/s), back_m, wait_minutes (game minutes for the last of them)};
`this_hour` and `hourly` (the last 72 hours, oldest first: {day, hour, cars, trucks, free, avoided,
revenue}). Saves from before this keep their booths with the new books empty.

```json
{"cmd": "select", "toll": 1}
{"cmd": "inspect", "toll": 1}
```

### The ledger: every account and every flow [ledger]

All money in the sim lives in accounts and moves only by one door (`World::transfer`), so nothing is
made or lost in town: every household's purse, every citizen's pocket, every business's till, every
property owner (firms, developers, residents' property companies, mega-corps' cash, the university's
funds), the tower developers, every tourist's wallet, the air taxi company, the independent landlords
(one pool for the buildings no owner record holds), the city treasury and one **outside world**
(wages earned out of town, imports and exports, visitors' money, owners out of town). Balances are
exact (whole 1/10000 dollars); the sum of every balance and the outside world is always zero.

`ledger {}` (also `money`, `books`): `balanced` (true while that sum is zero), `sum_units`,
`accounts`, `treasury`, `landlords`, `outside`, `untracked_total` / `untracked_moves` (money a money
field gained or lost outside the door: zero in a healthy city), `orphans` (moves naming an account
that had gone, paid to or from outside instead), `moves_today`, `drifts` (the last few untracked
changes: tick, account, amount, the last kind it booked), `today` and `yesterday` (`by_kind`: each
kind's total, how much came `from_outside` and went `to_outside`), `days_kept` (60 days are saved).

- `{account: "household 12:3"}` (also `"person ID"`, `"business ID"` with the building's id,
  `"owner ID"`, `"tourist ID"`, `"tower N"`, `"treasury"`, `"outside"`, `"landlords"`, `"airline"`):
  `balance`, `class`, `name`, `open`, and `recent` (the account's last 50 moves, 24 for a person or
  tourist, newest first: tick, when, kind, other account and its name, amount: + in, - out). Not
  saved: after a load the rings start empty.
- `{kind: "rent"}`: one kind of flow, `today` and each kept day, by payer and payee class (`flows`:
  kind, from, to, amount, moves). Kinds: the treasury's flows (`income_tax`, `business_tax`,
  `property_tax`, `bus_fares`, ...) and `wages_and_pensions`, `payroll`, `living_costs`,
  `pocket_money`, `rent`, `shop_rent`, `upkeep_and_running_costs`, `mortgages`, `profits_paid_out`,
  `shopping`, `outings`, `walk_in_sales`, `supplies`, `imports`, `exports`, `building_work`,
  `property_sales`, `investment`, `cars_and_bikes`, `private_fares`, `private_fees`, `theft`,
  `gifts_and_help`, `tuition`, `healthcare`, `tourism`, `grants`, `arrivals`, `departures`,
  `untracked`, `opening_balances`.
- `{day: 12}`: one closed day (game day): `by_kind`, every `flow`, and its `summary` (as `flows`).
- `{flows: true}`: the money-flow page's figures for `today` and `yesterday`: by group (property, sales,
  wages, profits, city, trade, other) each group's flows, and `in_from_outside`, `out_to_outside`,
  `net_with_outside`.
- `{trace: "business 4:1"}` keeps every move of one account (up to 100 000) until `{trace: null}`;
  `{account}` then returns them as `trace`. `{check: true}` reconciles every account after every tick
  and logs drifts (dev; slow in a big city; also the env var `ND_LEDGER_CHECK=1`).

Rent goes to the building's owner: a resident's purse, a company's account, a corp's cash, the
university's funds, or the independent landlords' pool. The owner pays 30% of it as upkeep (out of
town) and 10% as property tax (the treasury's `property_tax` line); residents and their property
companies also pay 15% to the bank. Businesses renting their unit (no owner, or a developer's,
university's or residents' company's building) pay `shop_rent` (a tenth of a home's rent per job) to
the building's owner. Developers send what they hold over $20,000 out of town each day
(`profits_paid_out`); the landlords' pool pays what it holds over $20,000, up to the last day's rent
after upkeep and tax, to the households with $5,000 or more saved, in proportion to their savings
(`profits_paid_out`, landlords → households), and sends the rest (the price of buildings it sold,
paid with the buyer's loan from outside) back out (`property_sales`, landlords → outside);
a residents' property company pays its founder's household half its day's net. A business pays its
staff's households their payroll wage (60% of the job's wage) itself (`wages_and_pensions`,
businesses → households); the rest of their earnings, and everyone else's (the city's jobs, building
sites, gigs, pensions), still comes in from outside, and only the payroll of staff with no household
leaves town (`payroll`). The household, business and owner inspectors show the account with its recent moves;
City Data, Economy, Money flow (`city_data {tab: "economy", sub: "money_flow"}`) draws the day's
flows from payers to payees.

```json
{"cmd": "ledger"}
{"cmd": "ledger", "account": "treasury"}
{"cmd": "ledger", "kind": "rent"}
{"cmd": "ledger", "flows": true}
```

### Industry sub-zones

`industry {lot | pos | polygon, kind}` sets the player's industry bias (`World::set_industry_bias`):
`kind` is `farming`, `oil`, `refinery`, `manufacturing`, `logistics`, `fishing`, `tech`, `mining`,
`smelting`, `factory_farming`, or `none`
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

### Dirty industry

Mining, oil, refineries (and chemical works), smelters and factory farms are dirty (see DESIGN,
"Dirty industry"). `inspect {building}` has `dirt`: `dirty` (for a dirty plant: `level`,
`reach_m`, `reads_as`, `spill` {radius_m, per_hour}), `dirtiness`, `as_neighbour` (the row of the
compatibility table it reads), `exposure` (0..1), `unhappy_neighbours` ([{building, name, what,
distance_m, hit}], worst first), `costs` ("food output −30%, land value −0.09") and
`ground_contamination`. `terrain_at` has `contamination` and `dirty_nearby` {plants ([{building,
name, what, distance_m, level, reach_m}] whose reach covers the point), home_exposure,
farm_exposure}; `dirt_at {x, y}` returns just those. `report {section: "industry"}` lists `dirty`
plants and the count of `suffering_neighbours`.

`remediate {x, y, radius}` (also `cleanup`) pays to dig out the contamination within `radius`
(default 100, at most 300 m) at 450 per cell of full contamination, booked as construction.
Reply: `cost`, `removed_cells`, `treasury`; an error when the ground is clean or the treasury short.

Testing: `contaminate {x, y, radius?, amount?}` pours contamination onto the ground (`radius`
default 60 m, `amount` 0..1, default 0.6). `paint_mural {building, group?, quality?, seed?}` puts a
mural on that building's street face now (`group`: a street-art crew id, default the first crew;
`quality` -1..1, default 0.6).

```json
{"cmd": "dirt_at", "x": 120, "y": -40}
{"cmd": "remediate", "x": 120, "y": -40, "radius": 120}
{"cmd": "contaminate", "x": 120, "y": -40, "amount": 0.8}
```

### Buses

`bus {action, ...}`:

| action | fields |
| --- | --- |
| `select` | `route` (omit to clear): highlight that line on the map, its path, stops and every bus on it, as clicking it in City Data's Services, Transit or clicking one of its buses does |
| `new_route` | (none) |
| `add_stop` | `x`, `y`, `route` optional (default: the line being drawn, or a new one); a click within 10 m of another line's stop shares that stop |
| `insert_stop` | `route`, `index` (the new stop's place: 0 before the first), `x`, `y`: a stop between two others |
| `move_stop` | `route`, `index`, `x`, `y` |
| `remove_stop` | `route`, `index` (the last stop removes the line) |
| `set_buses` | `route`, `n` |
| `set_fare` | `route`, `fare` (0..20) |
| `problems` | `route`: what keeps the line from running or is worth a warning, each `{kind, leg, blocks, text}`; kinds `too_few_stops`, `no_road`, `long_way_round`, `no_buses`, `no_depot`, `depot_unreachable`, `fleet_short`, `depots_full`, `out_of_service` (`leg` i is stop i to the next) |
| `check` | `points` [[x,y],...]: the problems a line through those stops would have, before it is built |
| `remove_route` | `route` |
| `rename` | `route`, `name` |
| `recolour` (or `recolor`) | `route`, `color` (0..7, the line and its buses) |
| `line` | `route`: the Lines window's figures: per stop `on_today`, `off_today`, `on_yesterday`, `off_yesterday`, the leg to the next stop (`leg_m`, `leg_min`, `leg_timed`, `leg_runs`, `leg_broken`); `length_m`, `lap_min`; `buses_out` with `state` (on_line, last_lap, from_depot, to_depot), `next_stop`, `progress` (in stops: 2.4 is 40% from the third stop to the fourth), `passengers` |
| `select_stop` | `route`, `index`, `look` (bool, point the camera there): mark the stop on the map as the Lines window's "show" does; no `route` clears it |
| `list` | every line's row with its `problems`, and `fleet {wanted, depot_capacity}` |

`line` also carries `headway_min` (buses leave the depot this far apart and are held at
stops to keep it), `problems`, and `books {riders_today, riders_yesterday, fares_today,
fares_yesterday, upkeep_yesterday, net_yesterday, bus_minutes_today}`: each line pays
for the bus-minutes it ran. `report {section: "ridership"}` explains who takes the bus and why
the rest do not.

Rows (`list`, and the `row` the edits return) carry `riders_yesterday` and each stop's
`on_today`/`off_today`. The Lines window is `window {name: "lines"}`.

[w1] `bus {action: "two_way", route, on}` runs a line out along its stops and back along the far
kerbs of the same streets (the way back's stops are made and kept in step with every edit; rows
mark them `back: true`, with `two_way` and `outbound_stops`); `on: false` makes it a loop again.

```json
{"cmd": "bus", "action": "add_stop", "x": 0, "y": 136}
```

### Subway

`subway {action, ...}` (also `metro`; `nd_app/src/remote/subway.rs`):

| action | fields |
| --- | --- |
| `status` (default) | the `subway` report section (below) |
| `station` | `x`, `y`: a station on the lot under the point (or the nearest usable lot within 80 m) |
| `line` | `stations: [id or [x, y], ...]` in order (a point picks the station within 120 m), or `points: [[x, y], ...]` (a station is placed on the lot at each point that has none within 120 m); optional `headway` minutes; `loop: true` (or the first station again at the end, three stations or more) closes the line into a loop. Pays the tunnel |
| `preview` | `stations` (as for `line`), `loop`: what the line would be, without laying it: `preview` {stops, looped, length_m, legs_m, run_min, round_trip_min, trains, headway_min, tunnel_cost, upkeep_per_day}, `affordable`, `treasury` |
| `headway` | `line`, `minutes` (4..60): minutes between trains each way |
| `remove_line` | `line`: its trains are taken off (riders set down at the next platform) and people waiting walk instead |
| `inspect_line` | `line`: the row plus `path` (every fourth point) and `trains_detail` (id, forward, at, passengers, dwelling, next_stop) |
| `inspect_station` | `station`: id or `[x, y]` |
| `insert_stop` | [w1] `line`, `station` (id or `[x, y]`), optional `index` (omitted: where it belongs along the line, or onto the nearer end): the tunnel is re-laid and only new stretches are paid for. Reply: `line` row, `index`, `paid`, `treasury` |
| `remove_stop` | [w1] `line`, `index`: a line keeps two stations (delete it instead) |
| `move_stop` | [w1] `line`, `index`, `station`: call at another station instead |
| `set_loop` | [w1] `line`, `on`: close into a loop (three stations or more) or open it |
| `relay` | [w1] `line`, `stations` (in order), optional `loop`: the whole line at once |

Line rows: id, name, colour, headway_min, looped, length_m, stations (id, name), trains, waiting,
riding, riders_today, riders_yesterday, runs_today. Station rows: id, name, central, entrance,
platform, lines, boardings_today, boardings_yesterday, waiting.

```json
{"cmd": "subway", "action": "line", "points": [[300, -440], [210, -160], [90, -25], [-90, 25], [-210, 160]], "headway": 8}
{"cmd": "subway", "action": "line", "stations": ["17:1", "18:1", "19:1", "20:1"], "loop": true}
{"cmd": "subway", "action": "preview", "stations": [[-243, -160], [160, -200], [250, 160]], "loop": true}
{"cmd": "subway", "action": "headway", "line": "1:1", "minutes": 6}
{"cmd": "subway"}
```

#### The metro line tool: `metro_line`

`metro_line {stations?, click?, loop?, undo?, finish?, cancel?, headway?, tool?}` drives the
game's metro line tool exactly as the player does (`nd_app/src/metro_ux.rs`), so the screen shows
it: the tool is picked (Build window, Transit, Metro lines), which switches the map to the Metro
layer, unless `tool: false`. Fields run in this order:

| field | does |
| --- | --- |
| `cancel` | drop the line being drawn first (Esc) |
| `stations` | `[id or [x, y], ...]`: each clicked in turn. A point picks the station whose lot it is on, or whose platform or entrance is within 75 m; a point on no station does nothing but say so (as a click off a station does) |
| `click` (also `add`) | one more station, as above |
| `loop` | click the first station again: closes a loop (three stations or more) |
| `undo` | `true` or a count: take back the last stop(s) (right-click / Backspace); a closed loop opens first |
| `finish` | lay the line (Enter / Finish), with `headway` minutes if given. Fails (400) with the reason when there is nothing to lay or the treasury is short |

A refused click does not fail the command. Reply: `clicks` [{at, station, taken, message}] (the
message is the status line the player sees: why a click was not taken, e.g. "Foundry is already the
last stop", "a loop needs three stations or more", "the treasury cannot pay the tunnel to Mirror
Lake: 695 m of line costs 15284, the treasury has 3598"), `draft` {stations [{id, name}], looped,
ready, preview (as `subway preview`), hover (the station under the pointer), tool_active,
stations_in_city}, `status`, `line` (the new line's row after `finish`, else null), `treasury`.

Tunnels are bored straight from station to station under everything, so any two stations connect;
only the order (the last stop again, a station already on the line, a loop of two) and the money
can refuse a click.

```json
{"cmd": "metro_line", "stations": ["17:1", [160, -200], [250, 160]]}
{"cmd": "metro_line", "click": [-160, 205], "loop": true}
{"cmd": "metro_line", "undo": 1}
{"cmd": "metro_line", "finish": true, "headway": 6}
{"cmd": "metro_line", "cancel": true}
```

### Finding and inspecting

`find {what, near?, radius?, filter?, limit?}`, with `what` one of citizens, buildings, lots, cars,
trains, metro_trains, routes, gestures. With `near` the rows are nearest first and carry `distance`; `radius`
defaults to 100 m then. Filters (substring match on strings, case-insensitive):

- citizens: `activity` ("commuting", "at home", ...), `name`, `class` (low / middle / high), `visible`, `spotlight`
- buildings: `land_use`, `tier`, `business_kind` ("grocery", "parts fab", ...), `name`,
  `under_construction`, `plant`, `rail`, `depot`, `derelict`
- lots: `land_use`, `vacant`, `park`, `min_area`
- cars: `kind` (car, bus, truck); trains: `kind` (passenger, freight). Train rows carry `headcode`;
  metro_trains rows: `id`, `headcode`, `line_name`, `pos`, `passengers`, `dwelling`
- bots (also `delivery_bots`): `shop` (the owning shop's name). Rows: `id`, `name` ("Lucky Bodega bot #2"),
  `shop`, `pos`, `returning`.

Reply: `total` (matches), `returned`, `rows`.

```json
{"cmd": "find", "what": "buildings", "near": [0, 0], "radius": 300, "filter": {"land_use": "commercial"}, "limit": 5}
```

`inspect {segment: id}` or `inspect {road: [x, y]}` (the piece nearest the point) gives a road piece: ends, kind,
lanes, `level` (ground / viaduct / cutting / tunnel / bridge), `levels` and `heights` at its ends, `inner_heights` (`[t, z]` samples between them on a long street over the hills), the
`bridge`/`viaduct`/`tunnel` flags, its nodes and their degrees, and `grade_crossings`: every way it
passes over or under (`passes`, `other_segment`, `clearance`). Also `condition` (null for pieces the
city does not maintain): `wear`, `condition`, `label` (good / fair / worn / crumbling),
`speed_factor`, `crash_factor`, `land_value`, `repair_cost`, `queue_place` and `queue_length` in
the crews' queue, and the `street` name.

`inspect {junction: [x, y]}` gives the nearest junction: its signal (phases, green, timer, protected
left arrow), each arriving street's lane arrows (`"L"`, `"S+R"` ...), recent turn shares, queues and
the mean and max left turn wait there. A car's `inspect` carries `traffic` (wait reason, lane arrows,
next turn, left turn wait, passing on the shoulder). `lane_use {junction: [x, y], approach: [x, y],
lanes: ["L", "S+R"]}` sets the arrows of the street arriving nearest `approach`, leftmost lane first;
`lanes: "reset"` goes back to the demand-chosen default.

#### Intersection editor: `intersection`

`intersection {at: [x, y]}` reads what the Intersection editor edits at the junction (or lone
crosswalk) nearest `at` (default: the one the editor has open); any of the fields below edits it,
in this order, and the reply is the state after:

| Field | Effect |
| --- | --- |
| `signal` | `auto` (the junction's shape decides), `signals` (forced on where two phases can run), `none` (unsignalised: drivers yield, left turns give way) |
| `greens` | seconds per phase, in the order `phases` lists them (5..90; `null` leaves one as it is), or `"default"` |
| `left_arrow`, `arrow_seconds` | the protected left at the start of a green: `auto` (when left turners queue or starve), `always`, `never`; its length 3..30 s |
| `left_turns` | `true` allows the left from every street, `false` forbids it from every street |
| `no_left` | `[approach index or [x,y], ...]`: those streets forbid the left, the others allow it. A banned left is taken out of what the junction offers there, so its lane arrows lose it and no route turns left there |
| `approach` + `lanes` | lane permissions of one arriving street (index, or a point on it): `["L+S", "S+R"]` leftmost first, or `"reset"` (as `lane_use`) |
| `crossing` + `upgrade` | a crosswalk (walk-graph index from `crossings`, or a point near it): `none`, `button` (a push-button signal: traffic gets a red only after someone presses it; not on a crosswalk that already has walk lights), `footbridge` (a raised walkway with stairs at both ends; refused where it would stand in another's way) or `removed` (also `remove`, `no_crossing`: [walk-crossings] no crossing there at all: nothing painted, no walk light, walkers route round by another crossing and nobody plans a jaywalk within 30 m; free). A footbridge replaces the crosswalk under it: its paint, walk lights and at-grade walk edge go, so everyone crossing there climbs over and nobody holds up traffic; [walk-crossings] it also replaces the other crosswalks over the same street within 30 m (not its own junction's other arms, nor ones the player set up), and no jaywalk is planned within 80 m of it. Unmarked crossings (a roundabout's own lanes, a highway slip road's mouth, a dead end) may take a button, a footbridge or be removed too: a button makes one a marked crossing with its own light, and a footbridge over a slip road's mouth has its stairs down each street's pavement. Paid for: 1,500 and 18,000 |
| `crossing` + `keep_crosswalk` | [footbridge] `true` keeps the crosswalk at grade under its footbridge (walkers then take the bridge when it costs them at most 30 m more); `false` (the default, also for footbridges from older saves) lets the bridge replace it. Free |
| `open`, `extent`, `select_approach`, `select_crossing` | open the editor (and the width in metres of the box it draws round the junction on the map, 30..300; which street or crosswalk is picked). [r1-roads] Time stands still while it is open (`status.paused`), as it was again when it closes |
| `lane_menu`, `hover_lane` | [pt-ux] `[approach, lane]` (lane 0 the leftmost): open that lane's options menu beside its arrow, or show its moves as if hovered (bright: allowed, dashed: offered but not allowed); `null` closes / clears. Needs the editor open |
| `close` | hide it |

[pt-ux] In the game the editor opens from the "Edit intersection" button that pops up beside a
junction or crosswalk under the pointer with the Edit tool (`tool {name: edit, cursor: [x, y]}` pins
it there), not from a click on the junction (which edits the roads like anywhere else). [r1-roads]
(K20) It edits on the map itself: a box is drawn round the junction, the camera comes over it, time
stands still and the other tools leave the map alone until Done (or Esc); the controls are a panel
at the right. On the roads in the box every arriving lane has an arrow at its stop line showing its
moves (a crosswalk's dot is picked before an arrow under it, K2); hovering one draws them
through the junction and clicking one opens its menu (the lane's options as arrows, the street's
automatic arrows, left turns from the street). Picking an option keeps every exit reachable: a
move no lane has any more goes back on the lane nearest its side. Beside the view: traffic signals
and the protected left turn as on/off toggles (each with "auto" to hand it back to the junction),
left turns on/off, the signal timing folded away, the picked crosswalk's upgrade, Reset junction
and Done.

Reply: `intersection` {`at`, `junction` (false for a lone crosswalk), `node`, `radius`, `roundabout`,
`signalized`, `rule` {signal, left_arrow, arrow_seconds, greens [{dir, seconds}], no_left [dir]},
`approaches` [{index, segment, street, dir (the way it leaves the junction), from, lanes, offered,
set_by_player, left_forbidden, left_possible, queued, lane_ends [{end, dir, moves [{move, allowed}]}]}], `phases` [{index, approaches, green_s,
running}], `crossings` [{index, mid, ends, upgrade, signalised, kind, button_light, footbridge
{walkway, stair_feet, width}, crosswalk_kept, crosswalk_removed, ramp, shut}], `footbridges` (in the city), `button_presses_total`}, `done` (what
was changed) and `editor_open`. Everything is saved with the city (`Traffic::junction_rules`,
`World::crosswalks`, serde defaults) and found again by position after the roads are rebuilt.

```json
{"cmd": "intersection", "at": [209, -149], "greens": [25, 14], "left_arrow": "always", "no_left": [2]}
{"cmd": "intersection", "at": [209, -149], "crossing": 700, "upgrade": "footbridge"}
```

The reply also has `level_crossing` (null unless the junction's node lies on the railway; see
below). At a level crossing that is no junction (a street straight over the track), `intersection`
answers with `intersection: null` and the crossing as `level_crossing`.

#### Walk networks: `walk_networks` [walk-networks]

`walk_networks {buildings? (8)}`: the walk graph's separate networks (places joined on foot), the
main one first (most buildings, then most walkways), each with `id` (kept across rebuilds: a network
takes the id of the largest network it overlaps from before, so a merge keeps the larger one's id and
colour), `main`, `name` ("around <its busiest building>", else "along <street>"), `color` (#hex, as the
map layer draws it), `color_linear`, `nodes`, `edges`, `underground_edges`, `buildings`, `residents`,
`workers`, `lo`/`hi`/`center` (its box) and `some_buildings` [{id, name}]. `layer` is the map layer's
key: `overlay {layer: "walk_networks"}` shows every walkway in its network's colour over a dimmed city,
the buildings off the main network outlined, the legend listing the networks by size (hover names
them, click flies there). Walkable: pavements, footpaths, crossings, footbridges, park walks, lot loops,
pedestrian tunnels (a footpath 5 m or more down) and street tunnels set to Mixed (walkways along the
bore; a car-only or older street tunnel is for cars alone). People in a tunnel are drawn down in it
and shown in the underground view; the crossings and level crossings of the ways above are nothing
to them.

#### Crossing use: `walk_crossings` [walk-crossings]

`walk_crossings {at?: [x, y], radius? (80), all?, reset?}`: every walk-graph crossing near `at` (or in
town) with `index`, `kind` (junction / zebra / informal / jay), `mid`, `segment`, `street`, `marked`,
`closed` (a footbridge replaced it, the player removed it, or a jaywalk in a footbridge's shadow),
`bridged`, `button`, and its use since the walk graph was last built (or `reset: true`): `at_grade`
(walkers who stepped out over it), `against` (of them against the light or mid-block), `bridge`
(walkers over its footbridge's deck), `bridge_bikes` (cyclists wheeling over it), `replans` (walkers
who found it gone and planned again). Unused jaywalking edges are listed only with `all: true`.
`totals` sums them (and `unmarked_at_grade`); `replans_total`, `jay_give_ups_total` (jaywalkers who
tired of waiting for a gap and went round by a crossing), `jaywalks_total`, `collisions_total`,
`near_misses_total`. Rules behind it: walkers never cross a highway slip road's mouth except with
walk lights (an off-ramp at a signalled junction), a push button or a footbridge (`shut` otherwise);
a roundabout's own lanes are crossed unmarked only when the island is where the walk goes (they cost
a route 60 m extra); jaywalking short cuts cost 12 m more per lane past the second and none are
planned over five lanes or more; a jaywalker waiting more than 5 ticks for a gap goes round by a
crossing. `report` traffic.pedestrians adds `replans_total`, `jay_give_ups_total` and
`hurt_in_the_street_now`; `report` transit adds `bike_rack_rides_total`, `bike_rack_full_total` and
`bikes_on_racks_now` ([bike-racks] a bus carries four bikes; a cyclist facing a long ride may take
the bus with the bike if the rack has room, else waits for the next bus).

#### Level crossings: `level_crossings` [rail-crossing]

Where a street or a footpath crosses the railway at grade. Streets of more than two lanes in all
get half-barriers (an arm on the arriving traffic's right reaching over its lanes to the centre
line, and a short gate over the pavement behind it), smaller streets flashing lights, a bell and
crossbucks only, footpaths lights and small gates. The lights start 15 s before a train reaches the
crossing, the arms start down 3 s later (5 s to come down), stay down while a train is over it and
2 s after the last one clears, then rise (5 s). While the lights are on, vehicles stop at the stop
line (one too close to stop goes on and clears it), walkers and cyclists at the edge of the track;
a train never runs onto a crossing that is not closed with its box clear of vehicles (it waits; the
count is `trains_held`).

`level_crossings` lists every crossing; `{at: [x, y], radius?}` (default 60 m) or `{index}` returns
the one as `crossing`. Each: `index`, `node`, `at`, `rail_dir`, `guard` (barriers | lights |
footpath gates), `barriers`, `lanes` (of the widest street over it), `reach_m` (half its box along
the track), `phase` (open | warning | lowering | closed | raising), `lights_flashing`,
`closed_to_traffic`, `barrier_down` (0 up .. 1 down), `train_over`, `eta_s` (the next train),
`trains_coming` [{train, seconds}], `vehicles_waiting`, `people_waiting` (this tick),
`trains_last_hour`, `trains_per_hour` (over the last day), `trains_total`, `closures`, `closure_s`
(the current or last closure), `average_closure_s`, `vehicles_held`, `average_wait_s` (per held
vehicle), `trains_held`, `arms` [{segment, away (the way the arm leaves the crossing), lanes_in,
lanes_out, path, stop_line_m (from the centre), barrier, barrier_length_m}]. Aliases `crossings`,
`level_crossing`, `rail_crossings`. The cycle and counts are saved (serde defaults); the geometry is
taken from the roads again on load. A vehicle stopped at one shows `why` `level_crossing`.

```json
{"cmd": "level_crossings"}
{"cmd": "level_crossings", "at": [0, -480]}
```

`plan` also carries `will_refuse` (null, or word for word what implementing the active project
would answer: a city structure its roads take, else no room for a raised road's columns), `blockers`
(each structure as `{id, name, at, road_gesture, road, stretch, runs_over, reason}`: the planned road
that takes it, the stretch of that road over it, or `runs_over: false` for one the rebuild would leave
with no street) and `pillar_gaps` (as in the tool state): the same check implementing makes, a trial
rebuild included, so nothing is refused that `plan` did not say.
A raised road's ends are planned where the network lays them: an end dropped on a road lands on it
at its level (the deck ramps down) and needs no column; an end left in the air needs one within
10 m, and is refused with "the road ends in the air over a road at (x, y) ..." when there is none.

A citizen's `inspect` carries `raised_level`: how high they stand on a raised walkway (a
footbridge's stairs or deck), 0 at grade, and `way_height`: on an elevated path, a viaduct or a
bridge, the height of its deck where they are (what they are drawn at, walk view too), null at
grade. Dev: `walk_to {citizen, to: [x, y], from?}` sends a
citizen out on foot along the walk graph (footbridges included).

`inspect` with one of `citizen`, `building`, `lot`, `car`, `drone`, `vehicle` (a private car's record: owner, model, place with minutes_here, limit_min, illegal, ticketed_tick, impounded {pound, out_of_town, release_fee, auction_in_min} and tow_job), `train`, `metro` (a metro train; see **Trains** below), `toll` (a booth id; see Taxes and tolls), `road` (a street piece: id or [x, y]; see Street races, raves and the roads), `route` (ids) or `gesture` (int). A building that is a parking office adds `parking_office` {officers, patrol cars}, a tow company `tow_company` {trucks, jobs_open, pound}, a lot the city runs `parking.city_run`; a tow truck's `car` adds `tow` {job, state, reason, vehicle, company, out_of_town}, an officer's car `patrol`; [services] a recovery truck going to a wreck (errand `Recover`) adds `recovery` {wreck, metres_to_go, amber, off_law_allowed (within 250 m), move (its off-law move: Lane, Overtake, Kerb, Wait), company, out_of_town, sent_tick}, a written-off car `wreck` {since_tick, incident, tow_due_tick, truck, truck_sent_tick, pushed_aside_m, flipped (0 upright, 1 side, 2 roof), righted_at_tick}, and a car easing past a crash on the shoulder `passing_hazard` (the crashed car's id). `parking_price {building, price}` sets the fee at a city-run lot. [boot] `tow_car {car | vehicle}` (also `tow`, `boot`) clamps a private car and calls the tow truck, as the inspector's "Tow this car" button does: `car` is a car on the road (it pulls over to a kerb slot, or the street's edge, is taken off the lanes, and its driver and passengers walk on), `vehicle` a car's record (parked at a kerb, or driving). Reply {tow_job, vehicle, company, out_of_town}; an error says why not (not a resident's car, off the street, already towed, leaving town). A clamped car's place has `booted: true`; the owner's mood drops, they remember it, and they pay the release fee at the pound as for any tow. The enforcement report adds `boots_today` and `boots_total`. [passengers] A car row of kind car carries `occupants` ([{id, name}]: everyone riding along besides the driver, `citizen`) and `vehicle` (its record); a passenger's activity has `driver`, `driver_name`, `in_car`, `car`, `to`, `purpose`. `report` traffic.private_cars adds `passengers` {riding_now, walking_to_a_car, cars_with_passengers, today, outings_today, carpools_today, school_runs_today, lifts_today, yesterday, total, fullest_car, drop_offs_pending}. `place` takes `parking_enforcement[:tier]`, `parking_small`, `parking_large`, `parking_garage` ([parking-fit] a surface lot (`parking_large`, `parking_small` alike) fills the plot it is put on, and the multi-storey `parking_garage` the same in decks: spaces, cost and upkeep go by the plot's area; one put next to a city lot of the same kind joins it);
with none, the current selection. The detail matches what the inspector and the person pane show:
A citizen's `inspect` also carries `vehicles` {bike, car {id, body, class, price, place: parked (home / building / parking lot / kerb / street (no space) / dealer, building, pos, since_tick) or driving (car id, errand)}} and `commute` {pain per mode with recent_trips, parking_pain, trip_mode_now, last_trip {mode, minutes, pain}}: what the mode choice weighs and what the daily habits (buy a bike, sell the car, buy one) act on.

- citizen: name, personality, activity (label, destination, car/bus/train ids), home/work, mood,
  satiety, class, income, rent, savings, groceries, car/bike, thought, memories (newest first)
- building: land use, tier, style (the style drawn, `nd_procgen::building_style`), floors, built day, construction (progress, days left,
  deliveries, materials), land value, rent, wage, condition, homes/jobs and the people,
  footfall, power (powered share, demand), parking, plant, depot, rail link, business (cash,
  takings, `industry`, `books`: the last 30 days' revenue and spend, oldest first, needs and sells
  with stock and prices), park
- lot: land use, polygon, area, land value and tier, `land_value_for_use` (as the lot's own land use feels it), `access` (as in `terrain_at`), zone, building, vacant/landlocked, footfall
- [land] building `land`: `owner` ({id, name, properties, spent, detail {kind: firm (business) |
  developer | household | elite (citizen) | builder (founder) | company (founder)}}, or {kind: independent}), `sale_price`, `lots` (every
  plot it spans, primary first), `merged`, `pending_lots` (bought, still clearing), `form`
  (mcmansion | mansion | merged | mega_hotel | museum | tower | strip_mall | mall | mega_mall | null), `pressure` and `pressure_floors` at
  its lot, `height_cap`, `wants_to_expand`, `owned_with` (every property of its owner),
  [p2-look] `mall` (a strip mall, mall or mega-mall; else null: `shops`, `let`, `tenants`,
  `parking_spaces`, `parking_used`, `reach` (how many times further than a street shop it draws
  shoppers from), `reach_m` (the distance they weigh, m), `units` [{role shop/anchor/foodcourt,
  floor, let, occupant null or {id, name, own | kind}}]) and `site` (a building over joined plots;
  else null: `plots`, `backland`, `area_m2`)

#### History, replays and the full dump [insight]

The sim keeps a bounded memory of what happened, saved with the city (`World::insight`,
`crate::insight`; a save from before it loads with none). It only records: nothing in it feeds back
into the game.

- **Vehicles**: every vehicle on the road has a trip log from the moment it took to the road: each
  street piece it drove onto (segment, direction, entry point and time), every re-plan (`why`:
  `lane_change` (could not get into the lane its turn needs), `full_exit`, `crash_ahead`, `stuck` (on
  one route too long, or a turn on it closed), `parking_hunt`, `errand` (a lorry, van or city
  vehicle on to its next stop or back to base), `redirect`, `new_route` (a bus at a stop, a show, a
  chase), `emergency`; `avoided` (where the lane it went round begins), `remaining_before_m` and
  `remaining_after_m` (null and `found_route: false`: it looked, found none and kept its route)),
  U-turns (`dead_end`: turned round where the street ends), rage U-turns, stalls of a minute or more
  (`seconds` in traffic seconds, `cause`: queued_behind, red_light, giving_way, exit_full,
  lane_change, pulling_out, crashed, junction_blocked, level_crossing, ...; `behind`: the car in its
  way), anger in 10% steps, crashes, and errand changes (the parking hunt's steps: `parking_search`
  with `tries`, `to_parking_lot`, `to_kerb_space`; `return_to_base`, `tow_job`, ...). A finished trip
  is kept with its driver (their `last_drive`), or for vehicles nobody in town drives in a ring of the
  last 300.
- **Citizens**: each trip set off (`set_off`, or `next_leg` after a ride: the mode chosen, its
  expected minutes, the `runner_up` mode and its minutes, `could_not`: better-ranked modes that could
  not be done), each change of activity (the legs: on foot or by bike, driving, waiting for and riding
  the bus, the train, the metro, a lift, flying; arriving home, at work, out), routes that failed
  (`no_route` with why), and life events: `arrived` in town, new `home`, `job`, `car`,
  `household`, `crime_victim`, `suspect`, `patient`, `crash`. About a day of an ordinary life
  is kept; older records drop off.
- **Buildings**: `started` (a site, or placed), `built`, `demolition` (why), `opened` (the business
  kind), `closed`, `reopened`, `owner` (the estate owner, or independent), `rebuilt` (tier and
  floors), `moved_in` / `moved_out` (households), `incident` (fire, crime, medical, crash; its id) and
  `resolved` (how), `renamed`.

Each record carries `tick`, `day`, `time`, `at` [x, y] and `event`. Logs are capped in bytes (a
trip on the road 3 KB, a finished one 768 B, a citizen's trips 224 B and life 192 B, a building
256 B); when one fills, its oldest street pieces go first, then its oldest records
(`records_dropped`).

`inspect {car: id, history: true}` adds `history`: the trip (`kind` private / lorry / bus / service /
taxi / through / fleet / other, `driver`, `vehicle`, `to`, `set_off`, `ended`, `minutes_on_road`,
`summary` {street_pieces, replans by why, replans_repeated (re-plans that repeat one of the last few
records, same reason round the same place: counted, not written again), u_turns, dead_end_u_turns, rage_u_turns, stalls,
stalled_min, stalled_by_cause_s, max_anger_pct, crashes, records_dropped}, `now` {at, wait,
stalled_s, anger_pct, route_left_m} while on the road, and `records`). A car no longer on the road
still answers with its last trip (`car: null`, `on_road: false`). `inspect {citizen: id, history:
true}` adds `history` {trips (each `set_off` record with `legs` (with `minutes`, and `waited_min` at
a stop), `ended` (the activity it ended in) and `took_min`), trips_today, failed_routes, life,
last_drive, current_drive, records}. `inspect {building: id, history: true}` adds `history`
{events}.

`replay {car | citizen, step_m?}` gives a trip's way as points: `points` [[x, y, tick], ...] (a
fractional game-minute tick, timed along each street piece), from where it took to the road along
each piece's lane (while the roads are as they were; after a rebuild the recorded entry points),
to where it is now; `length_m`, `events` (every record but the street pieces), `on_road`. A citizen
gives the drive under way, else their last. The selected car's thin "came from" line in the game is
drawn from the same record where the app's own breadcrumbs do not reach (after a load).

`get {kind, id}` returns every saved field of one thing, as the save holds it (`value`; slotmap
keys shown as "idx:version" ids). Kinds: citizen, building, lot, zone, household, car, vehicle,
train, facility, route, unit, incident (its number), drone, bot, metro, metro_line, owner, cab,
ship, tourist, segment and node (their numbers). `path` picks inside it (`"business.stock"`,
`"route.0"`). `get {field}` returns a field of the world itself, private ones too (`field: "?"`
lists them: economy, estate, services, insight, ...), with `path` to narrow it; answers over
`max_bytes` (4 MB) say so and list the keys to narrow by.

`query {kind, filter?, fields?, offset?, limit?}` lists a kind: `filter` {dotted path: value |
"substring" | {min, max}} (a string also matches an enum variant by name, e.g. `"activity":
"Driving"`; {min, max} on an array compares its length), `fields` [dotted paths] to return
(default: every field, as `value`). Reply {total, matched, rows: [{id, ...}]}.

```json
{"cmd": "inspect", "car": "134:757", "history": true}
{"cmd": "replay", "citizen": "88:3", "step_m": 10}
{"cmd": "get", "kind": "building", "id": "120:4", "path": "business"}
{"cmd": "get", "field": "economy", "path": "transit"}
{"cmd": "query", "kind": "citizen", "filter": {"activity": "Driving", "mood": {"max": 0.3}}, "fields": ["name", "home", "mood"]}
```

### Land market

`ownership {building}`: the selection highlight's data: `owner` ({id, name, detail} or null for an
independent owner) and `highlight`, every property the owner holds (the building itself first among
them, `selected: true`): id, name, land_use, pos, `outline` (the merged footprint for a building over
several plots), home, business, form. A billionaire's other homes and businesses are among them.

`estate {action?}` (also `land_market`):

| action | fields | reply |
| --- | --- | --- |
| `summary` (default) | | `stats` (buyouts_total/today, merges_total, branches_total, mcmansions_total, mansions_total, elites_total, priced_out_total, convoys_total), `pressure_samples`, `floors_histogram`, `elite_gate` {population, gini, top_over_median, open, cap, needs}, `elites` (name, net_worth, origin local/visitor, mansion and its pos, home, pos, activity, cars, escorts, police_escort, guards, properties, `convoy` {boss_car, boss_on_road, boss_pos, escorts [{car, pos}], police, guards_at}), `nimby` (center, radius, strength, mansion, price_out_left), `forms` (buildings built as a form, with lots and pending), `owners` (id, name, detail, properties), `reserved_lots` |
| `pressure` | `at: [x, y]` (optional) | at a point: pressure, mobility, floors, the lot's height cap; else every cell with pressure >= 0.5 as `[pos, pressure, mobility]` |
| `spawn_elite` | | dev: a billionaire now, skipping the disparity gate but not the quiet-land rules; the error says how many plots passed each test |
| `expand` | `building` | dev: that firm opens a branch or buys the plot next door now |
| `buyout` | `building` | dev: a developer buys it out and rebuilds bigger, if the new building is worth it |
| `drive` | `citizen`, `to` (building) | dev: send someone there (a billionaire's car is brought round, so the convoy forms) |

```json
{"cmd": "ownership", "building": "146:1"}
{"cmd": "estate", "action": "pressure", "at": [0, 0]}
```

### Several businesses in one building [tenants]

A building that can be let in parts (flats of three floors or more on a street, shop and office
buildings of two or more, a supertall's two-storey mall base) is cut into `units` when it opens:
shopfronts on the ground floor and ranges of upper floors. Its own business has the units its kind
and staff want; the rest are let to other businesses, each a **tenant**: a record of its own
(`tenant_of` its building) with its own id, kind, staff, books, account (`business <id>` in the
ledger), hours, sign and rent, standing on its building's lot.

- `inspect {building}`: `business` is the building's own business (or, with none, its first
  tenant's: the old field, kept), plus `tenants` [{id, name, kind, closed, jobs, workers, money,
  rent_per_day, area_m2, hours}], `units` [{place, floors [lo, hi], shopfront, area_m2, jobs,
  occupant "vacant" | "own" | "tenant", business (id), name, kind, vacant_days}], `units_vacant`
  and, on a tenant, `tenant_of` {id, name}. `business.rent_per_day` and `business.area_m2`: the
  rent it pays for its floor area.
- `list businesses` has every tenant with `tenant_of`, `rent_per_day`, `area_m2`; `list
  buildings` lists structures only, each with `tenants` and `units_vacant`.
- Rent: a unit's floor area times a share of a home's rent in the building per m² (so by tier,
  land value and condition), to the building's owner, at most half the day's profit.

### Mega-corps and universities [megacorp]

`inspect {building}` carries `brand` for a building a mega-corp or a university owns (null
otherwise): `kind` (`mega_corp` or `university`), `corp` / `campus` id, `role` (headquarters,
holding, staff housing; main building, dorm, administration, theatre, sports centre, frat house,
campus plaza), `rent_discount` for staff housing, and `brand`: `name`, `colour` (hex),
`colour_index`, `shape` (circle, ring, hexagon, diamond, triangle, square, chevron, shield),
`monogram` and `logo`: the 11x11 pixel mark as text rows (`#` shape, `x` letter cut into the
shape, `o` letter on empty ground, `.` empty), the same mark the renderer lights on roofs,
banners, the HQ crown and the fleet's trucks. `ownership` and `inspect.building.land` show the
owner's kind as `mega_corp` (with the business it grew out of) or `university`.

`corps {action?}` (also `corp`, `megacorp`):

| action | fields | reply |
| --- | --- | --- |
| `summary` (default) | | `corps` (id, brand, origin, origin_kind, founded_day, `hq` {id, name, floors, building, pos}, cash, holdings, holdings_by_use, industries, staff, `housing` {blocks, homes, staff_housed}, revenue_yesterday, market_share, bought_total, levy_total, properties), `campuses` (as `campus`), `rules` {allowed, max_share, levy, cap_per_corp, room}, `thresholds`, `biggest_companies` (flagship, name, kind, holdings, staff, revenue, qualifies), `stats` |
| `rules` | `allowed` (bool), `max_share` (0.02-0.25), `levy` (0-0.2) | the rules as set (clamped) |
| `promote` | `building` (a business) | dev: its company becomes a mega-corp now (brand, HQ site bought); the corp |
| `buy` | `corp` or `building`, `n` | dev: the corp buys what it would buy next, `n` times (cash topped up); `bought` |
| `finish` | `corp` | dev: HQ now if none, every building site of the corp finished |
| `drive` | `corp`, `lot` | road metres from the corp's HQ (the cached table its buys sort by) |

`campus {action?}` (also `university`): `summary` (default) lists `campuses`: id, owner, brand,
name, tier, pos, places, students, dorm_homes, funds, `annexes` [{id, kind, name, building}],
`pending` [{lot, kind}], expansions_total. `grow {building?, days}` (dev) runs the campus's daily
round `days` times with one annex each day and every site finished.

A University is placed like any service: `place {kind: "university", tier, x, y}` (Services; also `campus`, `college`; needs about 825 m² at its lowest tier).

```json
{"cmd": "corps", "action": "promote", "building": "88:1"}
{"cmd": "corps", "action": "rules", "max_share": 0.05, "levy": 0.1}
{"cmd": "campus", "action": "grow", "days": 10}
```

### Skyline: skyscrapers, supertalls, megatalls [supertall]

League towers rise over everything the ordinary growth builds: skyscrapers from 150 m, supertalls
from 300 m, megatalls from 600 m. A tower developer (fictional firm) with the money breaks ground on
the densest, dearest downtown land (commercial or residential, land value 0.6+, busy streets, 50+
floors of neighbours within 160 m) in a city of 1,200 people or more; a supertall needs two plots
(1,500 m²), a megatall three (2,600 m²), so it buys out the grown buildings next door (never the
city's, a home bought outright, a billionaire's or another tower) and builds a podium and tower over
one to three months. A record attempt aims 6-24% over the tallest standing or rising when demand and
money allow; the city's height limit, a district's and the clear approach round a vertiport (120 m
within 350 m) hold it.

`skyline {action?}` (also `towers`; `height_limit {limit}` sets the limit):

| action | fields | reply |
| --- | --- | --- |
| `summary` (default) | `limit_rows` (default 15) | `limit_m`, `vertiport_clearance`, `classes_m`, `record_m`, `tallest_planned_m`, `population_cap_m`, `demand`, `classes` (standing/rising per class), `towers` (id, name, height_m, league, design, developer, lots, pos, under_construction, progress, rank, land_use, homes, jobs), `ranking` (tallest standing: rank, id, name, height_m, league, pos), `records` (newest first), `news`, `developers` (name, capital, pull, owner), `totals` |
| `limit` | `limit` (m; null or 0: none), `vertiport_clearance` bool | the settings |
| `sites` | `force` bool | where a tower could go: lot, pos, score, value, neighbour_floors, limit_m |
| `start` | `height` (m), `design` deco_spire / bundled / twist / needle, `at` [x,y], `force` (default true: skip the money, demand, population and downtown tests; limits and site rules hold), `rich` | dev: break ground now; `tower` row, or why not |
| `finish` | | dev: clear the bought plots and top out every tower site now |
| `fund` | `amount` (default 1e9) | dev: money to every tower developer |

Building `inspect` carries `tower`: `height_m`, `rank`, `rank_label`, `height_limit_m`, and for a
league tower `league`, `design`, `design_label`, `developer`, `lots`, `pending_lots`, `site_m2`,
`floors`, `started_day`, `topped_out_day`, `record_bid`, `progress`, `days_left`. City Data's City,
Skyline page (`city_data {tab: "city", sub: "skyline"}`) shows the same.

```json
{"cmd": "skyline"}
{"cmd": "skyline", "action": "start", "height": 520, "design": "bundled", "at": [0, 0]}
{"cmd": "height_limit", "limit": 300}
```

### Selection and the player's UI

| Command | Fields |
| --- | --- |
| `select` | `citizen` / `building` / `car` / `vehicle` (a parked car) / `bot` (a delivery robot) / `train` / `metro` (a metro train) / `unit` (an emergency unit, see Emergency units) id, `toll` (a booth id, an integer), `road` / `segment` (a street piece: id or [x, y]), or `at: [x, y]` (picks like a click: on a train's car it picks the train; a metro train only with the Underground view on; a road piece when nothing else is there), or nothing to clear |
| `selected` | full detail of the player's selection (see below) |
| `flags` / `flag` | the player's "Flag for Claude" bookmarks (see below) |
| `viewer` / `building_notes` | [viewer] the building viewer and the notes left in it (see below) |
| `favourite` | `citizen`, `on` (omit to toggle). Favourites wear a gold ring and diamond in the world (their car, driving or parked, a gold box) |
| `gift` | `citizen` (default: the selected citizen), `amount` (default 1000): money from the treasury to their household (their savings if they have none), as the person pane's "Gift $1000". Refused when the treasury cannot pay. Booked as `gifts_to_residents`, itemised by name; they remember it. Reply `amount`, `household`, `household_money`, `savings`, `treasury` |
| `tool` | `name` (inspect, road, zone, edit, bulldoze, buildings (alias power), transit, rail, district; the building ones are what the Build window's items pick: road and its presets for Roads and Rail track, zone for Zoning and Parks, buildings for Power, Services, Transit placeables, Water and Attractions, rail for stations and yards, transit for bus and metro lines; the metro line tool is `tool {name: transit, kind: metro}`, see `metro_line`), `preset` (Path, Rail, Lane, Street, Avenue, One-way, Ramp, Highway, Roundabout), `diameter` and `ring_lanes` (roundabout), `ring_at` `[x,y]` (show the roundabout preview there as if hovered, until the mouse is used), `kind` (the active tool's sub-mode: road kind, for buildings a category power / services / transit / rail / attractions or any `place` kind, station / freight_yard, route / depot, land use or zone mode), `mode` (zone: polygon / brush / marquee), `radius` (zone brush), `unzone`, `industry` (zone, industrial: auto / farming / oil / ...), `sub_zone` (zone, commercial: auto / neighbourhood / retail / offices / leisure; `land_use` leisure or entertainment picks commercial with leisure), `tier` (buildings: low / mid / high), `land_use`, `lanes_fwd`, `lanes_bwd`, `curved`, `show_routes`, `route`, `selected_gesture`, `elevation` (the Road tool's level for the next point, m), `depth` (underground / ground / sky: the view layer; the Tunnel preset switches to underground, other presets back), `draw` `[[x,y,level?],...]` (the road being drawn, as if clicked, so its hologram shows: [r4-ramp] each point snaps as a click there does, an end onto any road at that road's height, a point between only onto a road at about its level; `level` stands in for the Level dial for that point), `cursor` `[x,y]` (where the pointer stands for that hologram until the mouse is used); also the Buildings tool's and the Zone brush/marquee's plot highlight), `hover` `[x,y]` (the same pin, named for the plot highlight: with the Buildings tool the site there (a lot, or the unzoned parcels a lot would be made of, as `place {check}`) gets its outline and light wall, green if a click would build and red with the reason if not, and the Build window's "Here:" line follows the pin; with the Zone tool's brush or marquee, the parcel being painted. The pin holds until the real pointer moves over the map). The reply's tool state has `elevation`, `elevation_label`, `point_levels`, `tunnel`, `depth`, `grade_warnings`, and (refreshed with the road preview, a frame later) `pillars` {count, at, buyouts (building names a pillar would buy out), empty_lots, gaps ([{from, to, end_in_the_air, over_road, level, reason}]: stretches with no room for a pillar), refusal}, and `intersection_editor` (where the editor is open, or null). With a `cursor`/`hover` pin set, the reply also has `hover`: {`at`, `ok`, `note` (what a click would do), `reason` (why it would be refused), `outline` (the plot's polygon)}, or null where the tool has no plot there (roads, tolls, rail pieces and docks have their own previews) |
| `overlay` | **Map layers are exclusive**: one shows at a time (as the top bar's Layer button), with its key in the legend. `active` a layer key or `none`: show it alone. `layer` (zoning, land_value, districts, traffic, traffic_types, danger, paths, walk_networks, people, pollution, crime, map, metro, parking, trails, density, construction, or a land survey layer, below; [w1] `construction` dims the city and outlines every building site (cyan going up, with a wire box to the planned height; amber coming down for a rebuild; red coming down for good) with a progress label; [pt-ux] `paths` (Pedestrian network) draws every walkway by kind and makes everyone on foot glow over a dimmed city; `parking` colours each street by the legal parking a driver finds (free kerb slots plus public lots within 220 m) and outlines every public lot by how full it is with a "P used/capacity" label; `trails` is a long exposure: the city nearly black and every mover leaving a streak that fades over about ten seconds; `density` is a heat map of how densely each plot is built (floor area over plot area until the growth model's land density lands); `traffic` and `crime` also dim the city and light up the vehicles they are about, `map` is the street map view with street names and pins; `metro` dims the city and shows the metro lines in their colours and every station as a named marker, and picking the metro line tool switches to it, leaving it puts back the layer before) with `on` (bool; omitted toggles): on shows it and hides every other, off hides it if it shows. `land` a land survey layer key (shown alone) or `none`. Older form, a bool per layer name: true shows that layer alone (several trues: the last listed in the registry wins), false hides it if it shows. `projects` (bool, or `layer: "projects"`) is the dim other-projects switch, not a layer, and is left alone by the rest. Reply: every flag layer and whether it is on, `land` (the survey layer shown, or null), `active` (the key of the layer showing, or null) and `projects` (also in `status.overlays`). New layers are added as one entry in `crate::map_layers::MAP_LAYERS` and appear here by their key |
| `lighting` | `follow_clock`, `hour` (0..24; setting an hour unpins lighting from the clock) |
| `heat_vision` (alias `heat`) | `on` (bool; omitted toggles). Drone heat vision: buildings, ground and roads go near-black, people, vehicles, fires and drones glow by temperature (lights and engines white-hot), with motion blur, scanlines, noise, a vignette and a reticle. Same as key H and the top bar's Heat button. Reply `heat_vision`; also `status.lighting.heat_vision` |
| `graphics` (alias `quality`) | `quality` auto / low / medium / high. Pins the graphics tier (Low: MSAA, no SSAO or haze, no rain or snow, street lamps only glow; Medium: TAA, light SSAO, real light from the 18 street lamps nearest the view at night; High: TAA, full SSAO, volumetric haze, big shadows, 40 lit lamps. Every tier: wet, glossy roads while it rains, fog tinted and thickened by climate and weather) or hands it back to the frame-rate throttler (Auto). Pin one before screenshots that must compare. Reply `quality`, `tier` (in force from the next frame), `frame_ms` (smoothed), `sim_ms`, `auto_steps`, `reason`, `effects`, `long_frames` ([saves] the last 24 frames over 500 ms, oldest first: `ms`, `sim_ms` the tick time in that frame, `tick`; loads left out: hitches to chase); also `status.render.graphics`. `ND_QUALITY` sets the starting setting, else [settings] the one saved from the Settings window |
| `window` | `name` a tool window: lines / view (also `land`, `layers`, `map`: the View window, once Land) / alerts ([pt-ux] the draggable Alerts window: every toast as it happened with a "go" where it has a place, and the city's problems now, the `report` alerts) / saves / settings ([settings] graphics, text size, speech bubbles, the controls) / help / dev / demand ([r4-leisure] the small Demand for new buildings window; its place and open state are kept in settings.ron), `open` (omit to toggle). Reply `open`, `windows_open`. Applied in the egui pass of the same frame, so a following `screenshot` shows it. The old report windows are City Data pages now: `name` city / economy / market / people / services opens that tab (at its last sub-tab), taxes / power / transit / emergencies / districts / tourism their page (Economy › Taxes, Services › Power, ...); `sub` (or People's old `tab` overview / concerns / social) picks a sub-tab, `open` false closes the view, omitted toggles, `expanded` is accepted and opens. The reply is `city_data`'s. `name: "build"`: the Build window, `open` (omit to toggle), `section` roads / zoning / parks / districts / power / services / transit / rail / water / attractions (opens it there); `status.build_window` {open, section}. A building tool picked with `tool` opens the Build window at its section, as its hotkey does. `name: "title"`: the title screen (above) |
| `city_data` | `tab` city / economy / market / people / services / directory, `sub` (the tab's sub-tab, below; omitted: the tab's last one this session), `open` (bool; a `tab` alone opens; `open: true` alone reopens where it was; `open: false` closes). No fields: the state only. Reply `open`, `tab`, `sub`, `title` (the page's), `tabs` [{tab, label, subs}]. The full-screen City Data view (top bar, or J; Esc closes; [pt-ux] it sits above every other pane while open; a name picked on a page closes it so the inspector shows, and opening a window from the top bar closes it; every table's headings sort it: click for biggest first or A to Z, again to reverse, a third time for the given order): **city** overview / land / districts / tourism; **economy** overview / cash_flow / ledger / money_flow ([ledger] where the money goes) / taxes / corporations; **market** goods / businesses / trade; **people** population / inequality / concerns / social / favourites; **services** power / transit / emergencies; **directory** businesses / companies (also `owners`, `firms`) / people (also `citizens`). Applied in the same frame's egui pass, so a following `screenshot` shows it; `status.city_data` says what shows |

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
| `wind` | what a wind farm makes on an ordinary day there (the map's wind field times the climate's wind, through the power curve) |
| `solar` | the sun a solar field gets (the climate's, a little more on high open ground) |
| `contamination` | the poisoned ground (0..1, the saved raster), trace yellow-green to poisoned purple; the legend reads the figure and the nearest dirty plant under the pointer. Aliases `toxic`, `poison`, `soil_contamination` |
| `best_industry` | each spot coloured by the industry kind the ground favours most |
| `best_farming`, `best_oil`, `best_refinery`, `best_manufacturing`, `best_logistics`, `best_fishing`, `best_tech`, `best_mining`, `best_smelting`, `best_factory_farming` | the chance a new industrial lot there takes the kind, from the sim's own industry weights (the player's area bias left out) |

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
- **toll**: as `inspect {toll}` (Taxes and tolls, above).
- **road**: the road card, as `inspect {road}` gives under `road` (Street races, raves and the roads, below).

A car's detail (`inspect {car}`, `selected`) carries:

- `label`: "Mara Voss's hatchback", "bus on Line 2", "delivery truck", "a sedan passing through".
- `why`: `reason` (moving, queued, letting_out, red_light, pedestrians, jaywalker, give_way, turning_left,
  roundabout, exit_full, lane_change, at_kerb, bus_stop, unloading, arriving, crashed, takeover,
  in_show, scene, at_work, level_crossing, unexplained), `text` (plain language, naming the vehicle
  or person responsible), `by` and `by_label` (that vehicle: select it to follow the queue; for
  `pedestrians` / `jaywalker`, the person on the crossing nearest the car, e.g. "Ana Ruiz on a bike"),
  `by_kind` (`vehicle`, `citizen` or null), `people_on_crossing` (how many, for `pedestrians` /
  `jaywalker`), `good_reason` (false only for `unexplained`: a car standing for no reason the sim
  knows, a bug worth reporting), `stalled_s`.
- `jam`: who it waits for, back to the head of the queue (`Traffic::jam_chain`): `length`,
  `cycle` (a gridlock ring), `head`, `head_label`, `head_reason` (a `why`) and `chain` (each car's id,
  label, reason, speed, pos, stalled_s; at most 60).
- `trip`: `elapsed_min` on the road vs `expected_min` (from the route at departure, free flow plus
  a little per junction), `over_ratio`, `can_rage` (private drivers), `anger` and `anger_peak`
  (0..1), `anger_cause` (`late`: the drive is taking too long; `lost_race`: a street racer who lost,
  sore on the drive home; null when calm), `patience`, `temper`. Street racers, takeover crews and
  moto packs are not timed while they are at it (no destination to be late for); a racer's drive
  home is timed afresh.
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

### The building viewer and its notes [viewer]

The building viewer (top bar **Viewer**, or X; Esc closes) shows one building at a time on a
turntable against a plain backdrop, as the game draws it (the same procgen mesh, materials, signs
and window lights), filmed on its own stage far from the map with its own lights, so its day/night
and lights switches change nothing in the city. The city runs on behind it; while it is open the
keys are the viewer's (left/right flip, up/down change the variant in the catalogue or flip in the
other lists). Three lists: **city** (every building standing; a tenant is listed under the building
it rents in), **catalogue** (every style the generator can draw, `nd_procgen::buildings::STYLES`,
each in seeded variants: the tiers whose zone menus offer it, or all three for a forced style,
times one to three heights, times 3 seeds) and **notes**. Filters: search words, `group` (a
catalogue section: Residential, Commercial, Industrial, Entertainment, Civic, Parks and beaches,
Parking, Power, Transit and rail, Docks, City services, Attractions, Industry trades, Shops and
trades, Homes and estates, League towers, Waterfront, Capsule district, Malls, Lantern Alley, Garden suburb, Harbour Pastel, Old Stone, Solarpunk Green, Glass Campus, Patchwork, Skyline Luxe), `form`, `land_use`,
`district`, `story`, `business` (a kind's label), `tier`, `noted` (only what has open notes).

`viewer` fields: `action` status (default) / open / close / toggle / select / next / prev / list /
screenshot; `source` city / catalogue / notes; `building` (id; switches to the city list), `style`
(key `PencilTower` or label "pencil tower"; switches to the catalogue), `variant` (0-based), `note`
(id; shows what it is about), `step` (int, as the arrows), `filter` {...} (null or "" clears one),
`clear_filters`, `go_to` (true: select the city building shown, fly the camera to it and close the viewer, as its "Go to it in the city" button); the look: `light` day / night (or `night` bool), `lights` (window lights and signs),
`auto_rotate`, `yaw` / `pitch` (degrees), `zoom` (0.25..4, 1 frames the building), `reset_view`;
`inspiration` (a picture of the `Inspirations` folder, by the end of its path, shown beside the
building, or `none`; the folder is `ND_INSPIRATIONS` or an `Inspirations` folder above the home
folder, the exe or the source tree); `path` (with screenshot: a .png, default
`screenshots/viewer-<ms>.png`). Reply: `open`, `source`, `position` / `shown` (in the filtered list),
`city_buildings`, `catalogue_styles`, `filter`, `subject` (a city building: `id`, `name`, `style`,
`style_key`, `group`, `form`, `land_use`, `tier`, `floors`, `district`, `story`, `business`,
`footprint_m2`, `pos`, `tenants`, `owner`, `state`, `homes`, `jobs`; a style: `style`, `label`,
`group`, `family`, `land_use`, `tiers`, `on_zone_menu`, `variant`, `variants`, `tier`, `floors`,
`seed`), `notes` (the notes on the subject: on the building, or on the style for every variant),
`note` (the picked note in the notes list), `view` {night, lights, auto_rotate, yaw_deg, pitch_deg,
zoom, size_m (the drawn bounding box), ready}, `inspiration`. `list` adds `total` and `rows`
(`offset`, `limit`). `screenshot` films the turntable image alone (1280 x 800) once the building
has stood a moment (about a second after a change; it opens the viewer); the ordinary `screenshot`
shows the window with the viewer.

**Notes** (`building_notes`, alias `notes`) are work items left in the viewer: one box per city
building and one per style. They are kept in `building_notes.ron` in the game's home folder (not
in a save: they are about the game, across cities) and written on every change. A note on a city
building names the game (`game`, the game id, and `city`), the building's id, name, place and
style, so it still reads once the building or the city is gone. Actions:

| `action` | Fields | Does |
| --- | --- | --- |
| `list` (default) | `status` open (default) / resolved / all, `kind` building / style, `building` or `style` (its notes only), `this_city` | the notes, newest changes last as written; each `id`, `target`, `text`, `by`, `created`, `updated` (unix s), `status`, `resolved` {unix, by}, `replies` [{unix, by, text}], and for a building `in_this_city` (it stands in the running game). Also `open`, `total`, `file` |
| `get` | `id` | one note |
| `set` (alias `add`) | `text`, and `building` (id) or `style` (key or label) for a new note, or `id` to change one's text; `by` (default claude) | the note |
| `reply` | `id`, `text`, `by` | adds an answer under the note |
| `resolve` / `reopen` | `id`, optional `text` (a closing answer), `by` | marks it done or open again |
| `delete` | `id` | removes it |

```json
{"cmd": "viewer", "action": "open", "building": "214:1", "light": "night"}
{"cmd": "viewer", "style": "pencil tower", "variant": 4, "action": "screenshot", "path": "screenshots/pencil.png"}
{"cmd": "viewer", "action": "list", "source": "catalogue", "filter": {"group": "Malls"}}
{"cmd": "building_notes"}
{"cmd": "building_notes", "action": "resolve", "id": 3, "text": "the roof pad sits on the roof now"}
```

### Camera

`camera` takes any of:

- `look_at: [x, y]` (orbit target), `distance` (12..2500), `yaw`, `pitch` (radians; pitch 0.08..1.5, or down to -1.2 in the Underground view, where the orbit may swing below the ground: it rises back over the horizon once the view leaves Underground);
- `x`, `z` (target coordinates; `y` is accepted for `z`);
- `follow`: a citizen or car id, or `{"citizen"|"car"|"building": id}`; the target tracks it every
  frame until the next `camera` command;
- `mode`: orbit / walk (in walk mode `look_at`/`x`/`z` place the eye, yaw/pitch turn the head).

Reply: the pose (`mode`, `target`, `distance`, `yaw`, `pitch`, `eye`), and `following`.
A `follow` here is the same follow as the inspector's Follow button (below).

`follow {citizen|car|bot|building|vehicle?, stop?, on?, pov?, fly?, view?, turn?, tilt?, zoom?, cutaway?}` is the inspector's **Follow**
button: the camera stays on the thing (default: the selection) until stopped. Pan (WASD, middle or
shift drag) and the F-key jumps are locked; rotate and zoom still work. In game a banner says
"CAMERA LOCKED" with **Stop following (Esc)** and a gold frame holds the map; Esc stops it (and
is used up: a second Esc clears the selection). `stop: true` or `on: false` stops it. `pov: true`
looks through their eyes (Walk view: a walker's eyes, a driver's seat, a robot's camera; right-drag
looks around and springs back; Tab in game), `pov: false` back to the orbit camera; a building has
no point of view. The camera flies in smoothly first unless `fly: false`. A followed car that parks
hands the follow (and the selection) to its driver on foot; anything else that is gone ends it.
Any `camera` command ends a follow. Reply `following`, `pov`, `at`, `flying`, `camera`, `view`,
`chase_capable`, `nudge` {turn, tilt, zoom}; `status.following` names what is followed (or null),
`status.follow_view` chase / high, `status.chasing` whether the chase camera has the view, and
`status.render.cut_meshes` how many scenery meshes wear the cutting material.

[chase-cam] Anything with a heading (a person, cyclist, car, bus, lorry, emergency unit, robot,
train, metro train, ship, aircraft, camper) is followed with a **chase camera** by default: low
behind it (a car about 3 m up and 7.5 m back, a walker at shoulder height 3.5 m back; buses,
lorries, trains, ships and aircraft further), looking the way it faces and a little ahead, swinging
round with it on the turns (smoothed, rate-limited; a walker standing still keeps the last heading;
a reversing car's camera stays behind its nose). The camera keeps above the ground and comes in
towards the subject where a hill is in the way; scenery between the eye and the subject
(buildings, decks, trees, lamps) is cut away (a dithered cone, see `render/cutaway.rs`). Chasing
into a road tunnel or the metro switches the Underground view on, and back off when it comes up or
the follow ends. `follow {view: "chase"|"high"}` (F in game) switches to the old high orbit follow
and back; `turn` / `tilt` (radians) nudge the chase camera round the subject as a right-drag or
Q/E does (it swings back behind after a moment), `zoom` sets its distance (times the default,
0.55..3; the wheel in game). `follow` on the thing already followed keeps the follow (no new
flight) and applies only `view` and the nudges. `cutaway: false` (dev, for comparing) chases
without cutting anything away; `cutaway: true` puts it back. A building, toll or road has no heading: the high
view.

`zoom_to {citizen|car|bot|building|vehicle?, at?, distance?, pitch?}` (alias `zoom`) is the
inspector's **Zoom to**: a smooth flight (0.7 to 2.2 s, a hop out over long trips) onto the thing
(default: the selection; a moving thing is tracked while the camera travels) or the point `at`.
`distance` defaults by kind (a person 32 m, a car 40, a bus or truck 55, a robot 22, a building by
its size); `pitch` 0.62. Ends a follow of anything else. The reply comes at once: `to`, `at`,
`distance`, `seconds`; screenshot after `seconds`.

```json
{"cmd": "follow", "citizen": "12:3", "pov": true}
{"cmd": "follow", "stop": true}
{"cmd": "zoom_to", "bot": "4:2"}
{"cmd": "gift", "citizen": "12:3"}
```

**Delivery bots.** `select {bot}` selects one, `inspect {bot}` (or `selected`) gives `id`, `code`
("B-4"), `name`, `serial` (its number in the shop's fleet), `owner` {building, name, business_kind,
`fleet_colour` (each company's fleet is painted its own colour), fleet_size}, `state` (delivering /
returning to the shop), `cargo` {resource, amount} (null on the way back), `destination`,
`customer` {building, name, household}, `pos`, `metres_left`, `minutes_left`, `minutes_out` and
`route` (the pavement ahead, sampled). The map draws the route in the fleet colour.

**Trains.** [train-inspect] `select {train}` (or a click on any car of a train with the Inspect
tool) selects a railway train; `select {metro}` a metro train (clickable in the Underground view).
`inspect {train}` / `inspect {metro}` (or `inspect` / `selected` with one selected) gives:
`id`, `headcode` ("1E07": 1 for passenger, 6 for freight; metro "M12"), `kind` (passenger /
freight / metro), `cars` (coaches or wagons behind the locomotive), `car_name`, `length_m`,
`colour_index`, `colour_rgb` (the body paint; metro: the line colour), `pos` (head),
`line_speed_kmh`, `held`, `on_branch`, a metro train's `line`, `line_name`, `looped`, `forward`;
`state` {kind: running / dwelling (`at`, `at_name`, `leaves_in_s`) / held_signal (`by`, the train
in the block ahead, and its `by_headcode`) / held_branch (waiting at the switch for the single-track
branch) / leaving (all calls made, running out), `text`}; `run` {calls_made [{id, name}],
standing_at, calls_ahead [{id, name, eta_min (sim minutes, at line speed with the stand at each
call; signals can make it later), eta_time ("14:32"), pos}], next_stop, origin, terminus (first and
last stop), end_of_run {pos, eta_min, eta_time}, route_m, run_m, progress (0..1), route_ahead (the
track still to run)}. Passenger and metro trains add `passengers` {count, capacity, riders [{id,
name, alights, alights_name (their station), going_to, going_to_name (their final destination),
why ("going to work", "shopping for groceries", ...)}]}. Freight trains add `freight`
{import_units, unloaded (units already set down), by_kind [{resource, amount}], loads [{resource, amount, price, from ("out of town (by
rail)"), yard, yard_name (where it comes off), hub, hub_name (a cargo hub it passes through), buyer,
buyer_name, purpose ("material for the protein vat to make meat", "stock for the grocery's
shelves", "building materials for the site at ...")}], exports_aboard (units of the town's exports
picked up), exports_to_collect [{yard, yard_name, amount}]}: the imports a freight train shows are
those queued at the yards still ahead of it (they come in on it and are set down when it calls).
In game the inspector shows the same, and the map draws the selected train's cars boxed, the rest
of its run as a glowing band in its colour with posts at the stops ahead (the next one tallest,
with a pulsing ring), the track already run faintly, and an "o"/"x" at the ends of its route.
`follow {train}` / `follow {metro}`, `zoom_to {train}` and `camera {follow: {train: id}}` work on
trains (the camera rides the locomotive).

**A person's places.** A selected citizen's `holdings` are their home, work and every business they
own (a taxi firm's founder owns it: `business.owner`); in game each is a big clickable pin (HOME,
WORK, OWNS) over its building, held at the screen edge when off screen, and a button in the person
pane; clicking either selects the building.

```json
{"cmd": "camera", "look_at": [0, 0], "distance": 400, "pitch": 0.9}
```

### Screenshot

`screenshot {path?}`: queues a capture of the window (`Screenshot::primary_window()` with
`save_to_disk`) and replies at once with the absolute `path` (default
`<working dir>/screenshots/shot-<unix ms>.png`). A given path must end in `.png` and lie in
the allowed folders (see [Who may call](#who-may-call)). **The file is written a frame or two later**,
when the GPU copy lands: poll for it (the MCP tool waits up to 5 s). It captures the whole window,
UI panel included, at the window's size (a full-HD window makes a 2–5 MB PNG). PNG only. A
minimised window renders nothing and produces no file.

### Sound [sound]

The game's sound is synthesised in code (`crates/nd_audio`: no samples) and never touches the sim:
three ambience beds crossfaded by camera height (high: the distant city's wash and wind; mid: the
traffic roar; street: tyres and pass-bys, the crowd's murmur and footsteps, air-conditioning hum,
neon buzz), with rain, birds by day or crickets at night over green, water lapping, all muffled by
snow or the Underground view; and short UI cues. `ND_AUDIO=0` runs without opening the sound card
(agents' test instances: please set it).

| Command | Fields | Reply |
|---|---|---|
| `audio` (`sound`) | none or `get` | `settings` {master, ambience, effects, ui, music (0..1 slider positions; gain is the square), muted, unfocused: mute\|duck\|keep}, `status` {playing, device, sample_rate, channels} or {playing: false, why}, `focused`, `bus_gains` (after master, mute and focus), `beds` {weights {high, mid, street} (equal power), level_db per group {high, mid, street, weather_nature} (RMS before the bus), target (the bed parameters the camera's surroundings call for)}, `heard` (what was sampled: height_m, radius_m, cars_close/near, car_speed_ms, walkers_close/near, density, neon, green, industry, water, daylight, rain, snow, wind, underground), `voices` {ui, spot, ui_cap, spot_cap}, `cues_played`, `output_peak_db`, `limiter_gain`, `audio_thread_cpu_pct` (callback time over buffer time), `frames_rendered` |
| `audio` | `set` {master?, ambience?, effects?, ui?, music?, muted?, unfocused?} | changes them (saved in `settings.ron` a second later, like the Settings window's sliders); replies as `get` |
| `audio` | `play: "<cue>"` | plays a UI cue through the rate limiter: click, tool_change, road_point, road_done, implement, building_placed, bulldoze, zone_paint, window_open, window_close, error, alert, speed_up, speed_down, pause, resume |
| `audio_capture` | `path?` (.wav, default `exports/wav/capture-<unix ms>.wav`), `seconds?` (0.5..60, default 6), `as_set?` | renders what the camera hears now (the beds as last sampled; at the default sliders, or with `as_set: true` the player's; no focus ducking) offline with the same engine: path, peak_db, rms_db, dc, clipped, correlation, bands_db (octave bands 63 Hz..16 kHz) |

The beds are sampled four times a second around the spot under the camera; the engine eases every
change over a second or so. The UI cues come from watching what changed (tool, road points, project
steps, implement, buildings, refusals, speed and pause, windows, City Data, the selection, toasts,
and egui's button clicks); each has a shortest gap, repeats play softer, at most 10 a second, and a
click gives way to the cue it caused. Offline renders of every cue and bed state:
`cargo run -p nd_audio --example render_wavs -- <out dir>`.

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
- street nights: `News` (`seq`, `event` "street race" / "warehouse rave", `headline`, `pos`): a
  race gathering, the flag dropping, the winner, a bust or a chase; a rave opening, a noise
  complaint, a shutdown or dawn (see [Street races, raves and the roads](#street-races-raves-and-the-roads));
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
`export_income_today`, `import {max_mw, price_per_mwh` (this hour's), `mean_price_per_mwh}`, `export {mw,
max_mw, price_per_mwh, mean_price_per_mwh, income_today, income_yesterday}`, `battery_totals {count,
charge_mwh, capacity_mwh, level, charging_mw, discharging_mw, round_trip_efficiency}`, `weather` (as the
`weather` command), `grid` (as the `grid` command), and a row per plant and battery
(`World::plant_rows`, the same figures as City Data's Services, Power):

- `plants`: id, name, kind, capacity_mw, available_mw (what the weather allows), output_mw (less
  curtailment), mean_mw_24h, upkeep_per_day, cost_per_mwh (upkeep over the last 24 hours' energy; null
  when it made next to nothing), co2_t_per_day, co2_t_per_mwh, air_index (smog at the fence now, 0..1),
  noise_db (null while idle), noise_db_running, pollution_radius_m, pollution_land_value_effect,
  workers, jobs, derelict, pending (lot still being cleared), pos, `availability` (share of nameplate
  the weather, hour and site allow now), `site {wind_factor, sun_factor}` (wind and solar only), area_m2, size_scale, `group`
  (bool), `members`, `lots`, `member_names`, `output_mw_hours` (the last 72 hours, a group's
  members summed) and `energy_mwh_days` (up to 90 days). Touching solar lots (an edge, or a lot
  line up to 2.5 m wide, never across a road or path) are one row, a "<Name> Sunfield", and
  touching wind lots a "<Name> Wind Park" (`World::plant_groups`; derived from adjacency, not
  saved): the figures are the members' summed, each member making 4% more per extra lot (up to
  12%) for the shared inverters and grid connection; the id is the anchor member's. `inspect
  {building}` on any member has `plant.group {name, anchor, members, member_names, lots, area_m2,
  capacity_mw, output_mw, upkeep_per_day, bonus}`; `list {what: plants}` has `area_m2` and
  `group` (its name) per lot.
- `batteries`: id, name, kind, capacity_mwh, max_mw, charge_mwh, level, flow_mw (+ delivering,
  - charging), state (charging / discharging / idle), delivered_mwh_24h, upkeep_per_day,
  cost_per_mwh, noise_db, derelict, pending, pos, `flow_mw_hours`, `charge_mwh_hours`.

`status.power` carries the headline figures plus `exported_mw`, `export_income_today`,
`import_price`, `export_price` (this hour's, per MWh), `batteries
{count, charge_mwh, capacity_mwh, charging_mw, discharging_mw}` and `rows {plants, batteries}` (the
rows without the hourly series); `status.weather` is as the `weather` command. `report {sections:
["history"]}` hours gain `import_price` and `temp_c`, and `exported_mw`,
`curtailed_mw`, `battery_in_mw`, `battery_out_mw`, `battery_charge_mwh`, `battery_capacity_mwh`; days
gain `exported_mwh`, `battery_in_mwh`, `battery_out_mwh` and `ledger.grid_export`.

### report

`{"cmd": "report", "sections": ["overview", "alerts"]}` (default: all but `history`).
`{"cmd": "report", "sections": ["history"], "history_days": 90}` for the sim's history rings.

| Section | Fields |
| --- | --- |
| `overview` | day, time, population, households_housed, homes {total, free, being_built}, jobs {total, filled, free, being_built}, buildings {built, under_construction, derelict, vacant_lots, unusable_lots, lots_total}, treasury, net_yesterday, net_today_so_far, land_value_avg, gini, demand_by_land_use (0..1), demand_met_by_sites ([w1] the demand the sites going up already meet, 0..1), commercial_demand ([r4-leisure] neighbourhood, retail, offices, leisure and overall, 0..1: the Demand window's commercial bar and its sub-zones; `leisure` in the land-use maps is the old `entertainment`), headline_alerts |
| `people` | by_class, by_activity, employment {employed, employed_in_businesses, employed_on_building_sites, unemployed_on_gig_income, unemployment_share (of everyone), jobs_free, working_age_adults, unemployed_adults, adult_unemployment_share}, housing {housed, unhoused, waiting_immigrants, homes_free}, income_per_day and savings (distributions; savings also in_overdraft, at_overdraft_floor), rent_burden {over_30pct_of_income, over_50pct_of_income, their shares, rent_stressed_now, rent_to_income distribution}, mood (distribution 0..1), hunger {hungry_now, mean_satiety, no_groceries_at_home, hungry_days histogram 0..4+}, commute_modes_now (walk/bike/car/bus/train counts and shares of the people travelling now; owns_car_share, owns_bike_share), moves (displaced, emigrated, priced-out totals), recent_memory_reasons (memories of the last 2 days grouped by their fixed text, e.g. "Rent got too steep", "Couldn't afford anything at") |
| `economy` | treasury, ledger {today_so_far and yesterday by flow, nets}, ledger_items {yesterday, today_so_far: per flow {total, items: [{building + name / route + name / class / land_use / name, amount, postings}], not_itemised}} (City Data's Economy, Ledger drill-down: service upkeep by station, clinic and car park, transit upkeep by depot and the bus fleet, plant upkeep by plant, fares by line and station, income tax by class, business tax by land use), tax_today, upkeep_today, trade_today {imports_value, exports_value, building_materials_imported, deals, meals_sold, walk_in_sales, parking_fees}, businesses {total, open, closed_ever, by_kind: count, open, closed, mean_staffing, mean_money, revenue and spend totals, inputs (stock, target, fill), outputs (stock, mean_price)}, resources (town_stock, avg_price, price_vs_base, local_sellers_open, local_supply_per_day, business_demand_per_day, scarce), most_profitable_today and least_profitable_today (10 each, with ids) |
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

### directory [directory]

`{"cmd": "directory", "kind": "people", "sort": "income", "desc": true, "search": "kessler", "class": "middle", "district": "Old Town", "offset": 0, "limit": 50, "show": true}`

The rows of City Data's Directory tab (`city_data {tab: "directory", sub: "businesses" | "companies" | "people"}`),
sorted and filtered by the page's own code (`crate::directory`), so they match what the player
sees. `kind`: businesses (default), companies ([companies] every owner; also `owners`, `firms`) or people. `sort`: a column key, below (default name); `desc`
reverses; missing values (no rating yet, no job, no district) sort last either way and ties fall
back to the name. `search`: words that must all appear (case-blind) in the row's name, kind,
owner, street, district, state (businesses) or name, workplace, home, street, district,
favourite place, status (people), name, what it is, founder, district, biggest building and
its whole history (companies). Pickers: `district` (all), `business_kind` (a kind key) and
`state` (businesses), `class` and `status` (people), `owner_kind` (companies: builder, firm,
founder, property, corp, university, billionaire, family, developer). `show: true` also opens the page with these
settings. Reply: `kind`, `sort`, `desc`, `total`, `matched`, `offset`, `rows`, `took_ms`.

- Businesses' columns: `name`, `kind` (key) and `kind_label` ("Mill · farming"), `owner` and
  `owner_kind` (chain, developer, billionaire, person, household, mega-corp, university;
  empty for an independent), `district`, `street` (the street the lot fronts), `staff`/`jobs`,
  `revenue_today`, `spend_today`, `revenue_yesterday` and `profit_yesterday` (the closed books;
  null on the first day), `money`, `rating` (1-5 stars, null before anyone rated it), `rated`,
  `footfall` (passers-by today), `state` (building, shut = out of business, broke, no_staff, open,
  closed = outside its hours), `pos`. Sort keys: name, kind, owner, district, street, staff,
  revenue_today, revenue_yesterday, profit_yesterday, money, rating, footfall, state.
- People's columns: `name`, `age`, `class` (low, middle, high, elite), `income`, `work` and
  `work_label` (the workplace, or unemployed / child / retired), `home` and `home_label`, `street`
  and `district` (of the home), `household` (size), `mood`, `satiety`, `hungry_days`, `ill` (waiting
  for an ambulance or in hospital), `commute` (the mode they have used most lately), `favourite`
  (their best-liked place), `friends`, `status` (working, at_home, asleep, out, travelling, ill,
  homeless, arriving, away), `pos`. Sort keys: name, age, class (then income), income, job, home,
  household, mood, health (ill lowest, then how well fed), commute, favourite, friends, status.
- [companies] Companies' columns: `owner` (id), `name`, `what` ("local builder and landlord",
  "mega-corp from grocery (groceries)", "taxi company firm", ...), `owner_kind` (as the picker),
  `founder` (the resident behind it, by name), `properties`, `value` (what its buildings would
  sell for), `rent_yesterday`, `takings_yesterday` (its businesses'), `staff`, `vehicles` (cabs,
  tow and garbage trucks, lorries and vans on the road now), `since_day`, `district` (of its
  biggest building), `livery` (its fleet colour, "#e12832", or null), `flagship` (its HQ or
  biggest building), `pos`, `size` (value plus a year of rent and takings: the default sort,
  biggest first) and `history` [{day, text}] (founded, bought, grew, renamed, merged, sold up).
  Sort keys: name, what, founder, properties, value, rent, takings, staff, vehicles, since,
  history.

### export

`{"cmd": "export", "path": "exports/optional.json"}` writes `report` (all sections), `map_summary`,
`history` and `tables` (every row of every `list` kind) as one JSON document, by default to
`<working dir>/exports/city-<unix ms>.json` (`/exports` is git-ignored). A given path must end in
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
patient, taken_to), damage (building, name, condition_lost, destroyed), chase (null, or vehicle,
started, ended, result "tagged and arrested" / "lost them" / "called off", metres, closest_m: a
suspect who fled by car, stage "car chase" while it runs); plus jail_occupancy,
held_at_stations, released_early_total, today's out-of-town calls and fees, and chases_now,
chases_today, chases_tagged_today, chases_lost_today, chases_total, chases_tagged_total
(`report` has the same under `services.emergency.car_chases`). The Emergencies
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
`incident` (its id, as in the report and the events). [fire-scene] A fire takes `intensity` (dev,
0..1) to start it that big: one engine knocks off 0.012 a minute, so 0.8 keeps it busy about 70
minutes. `incidents {kind}` filters by fire, crime or
medical (crash victims are medical incidents).
`incident {kind: "crash", car? | at?: [x, y], speed? (m/s, default 25 = written off)}` crashes a car
(the next tick turns it into injuries and 911 calls); `incident {kind: "takeover", at?: [x, y]}`
starts a street takeover at the junction nearest `at`; `incident {kind: "moto_pack", at?: [x, y]}`
starts a moto pack ride meeting at the road end nearest `at` (car `inspect` shows
`takeover.moto_pack: true` for its riders).
`incident {kind: "chase"}` stages a car chase: a free police car comes for someone whose car is
parked near home (a robbery on the record) and they run for it; `chases` in `incidents` follows it.

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

### Emergency units and the fire scene [fire-scene]

`units {kind?, out?}` lists every fire engine, police car and ambulance: `unit` (its id), title
("Engine 2 · Eastgate Fire Station"), kind, state, doing ("on scene at Mira Tower fire, 12 min",
"responding to a fire at ...", "packing up at ...", "on patrol, parked on the beat"), pos (null in the
station) and incident. `kind` filters (fire_engine, police_car, ambulance, or Engine / Car / Medic),
`out: true` keeps the ones out of the station.

`select {unit}` selects one (driving, or standing on scene or on a beat), as a click does: a click
on a unit's car or on a unit standing still picks the unit (Selection kind `unit`), and a click right
on a firefighter at work picks that crew member (a citizen). `inspect {unit}` (or `inspect` /
`selected` with one selected; `inspect {car}` of a unit's car adds `unit`) gives: unit, title, kind,
call_sign, station, station_name, state, doing, lights, packing, car (while driving), pos, heading,
crew_size, crew (citizen, name, and at a fire role pump operator / on the hose / on the ladder, out,
pos [x, height, y], pose stand / nozzle / pump), incident (id, kind, stage, place, building, name,
minutes_on_scene, outcome; a fire adds intensity, how_bad smouldering / burning / well alight /
fully involved / out, minutes_to_out at one engine's rate, progress 0..1 knocked down, damage; other
calls add patient, suspect, people), hospital (ambulances), and for a fire engine at work `scene`:
incident, building, seconds on scene (sim time at 1x), water_litres (950 L/min on the hand line,
1,900 on the ladder), aim [x, height, y], ladder {yaw_deg (from straight ahead), pitch_deg, length
(7 stowed to 26 m), outriggers 0..1, tip}, hoses, streams (from, to, flow 0..1).
`camera {follow: {unit: id}}`, `follow {unit}` and `zoom_to {unit}` take a unit too.

What is drawn: when an engine stops at a fire its crew climbs out and walks to their places (pump
operator at the panel on the building side, the nozzle crew to a hose line in front of the
building, the ladder crew up the turntable); the outriggers go down, the roof ladder raises, turns
and extends towards the fire (the roof line, or on a tower too tall to reach, the highest floors it
reaches, where flames show in the windows) and the ladder crew climbs it to spray from the tip.
Water arcs from both nozzles (fewer droplets at the Medium tier, a thin stream at Low); a fire under
the hoses smokes less and steams more as it weakens. When it is out the engine packs up for
`PACK_UP_TICKS` (3) minutes (`packing: true`, still on scene and busy: water off, crew back, ladder
down) before it drives home. The scene is worked out from the sim each frame (`fire_scene.rs`),
never stored; a crew member out with a unit says so in their activity ("out with Engine 1: on scene
at Lotus Towers") and the camera follows them to the scene.

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
| `list` (default) | | `districts`: id, name, policy, residential, commercial, industry, historic, height_limit, landmark, area_m2, centroid, lots, vacant_lots, buildings, under_construction, by_land_use, by_tier, in_look (buildings already in the district's look), homes, residents, jobs, workers, businesses_open, hotels, beds, tourists_staying, land_value_avg, rent_avg, attraction; [stories] `story` (key or null), `story_label` (the story, "Custom" or "No story"), `closest_story` (a custom district's nearest story), `policies` {rent_support, nightlife_levy, late_hours (0/1), pollution_levy, heritage_grant, business_rate, bed_levy}, `policy_money_today` / `policy_money_yesterday` (by policy; positive to the city, negative paid by it), `levied_yesterday`, `spent_yesterday`, `growth_pull` (land uses whose new buildings go up more (>1) or less readily) |
| `stories` | | [stories] `stories` [{key, label, backstory, sets, planned_look}], `policies` [{key, label, range, help}] |
| `create` | `name` optional (generated otherwise) | `id`, `name` |
| `paint` | `id` (or `district`: id or name; none starts a new district, `new_name`), then `at` `[x,y]` + `radius` (default 40), `polygon`, or `rect` `[[x0,y0],[x1,y1]]`; `erase` | `id`, `cells_changed` (16 m cells) |
| `erase` | `at` + `radius`, `polygon` or `rect` | `cells_changed` |
| `set` | `id` / `district`; [stories] `story` (a key or label from `stories`, or none: its homes and shops looks, industry, Historic, height limit and policies replace the district's), any policy dial by key (`rent_support` 0–0.5, `nightlife_levy` 0–0.2, `late_hours` bool, `pollution_levy` 0–0.2, `heritage_grant` 0–100 $/building/day, `business_rate` −0.05–0.10 (below 0 relief), `bed_levy` 0–0.2; clamped); also (older, makes the district custom) any of `residential` kowloon / suburban / eco / capsule / brick_steel / lantern_alley / garden / harbour_pastel / solarpunk_green / glass_campus / patchwork / skyline_luxe / none, `commercial` neon_strip / tech_campus / night_market / tourist / capsule / brick_steel / lantern_alley / garden / harbour_pastel / solarpunk_green / glass_campus / patchwork / skyline_luxe / none ([capsule] white domes, saucers and pill towers; as `commercial` it opens offices and gadget shops only where the market carries them, and gives nights out its domes too; [lantern] `lantern_alley` is the Lantern Lanes story's look: shophouses and alley walk-ups, stall rows, food courts, tea houses and night-market halls, and as `commercial` it also dresses the district's industry as workshops and back-alley kitchens; [r4-harbour] `harbour_pastel` is The Promenade's look: pastel terraces, converted boathouses, lighthouse corner blocks, boardwalk cafés, seafood shacks, a striped-awning arcade and boutique hotels; a tourist quarter like `tourist`, and as `commercial` it dresses the district's industry as the fish market hall and net sheds; [r4-solarpunk] `solarpunk_green` is the Eco Village story's look: timber-and-glass terraces under planted roofs, solar-shingle townhouses, vine-covered towers, a glass market hall with a roof farm, co-op stores and repair cafés (a bike shop builds the repair café there), and as `commercial` it dresses the district's industry as vertical farms (a vertical farm business builds one there), greenhouse rows and a cargo-bike depot; farms keep their farmsteads; [r4-glass] `glass_campus` is Founders' Campus's look: student flats with bright panels, co-living towers with lounges lit all night, glass labs in exposed steel, start-up sheds, atrium offices and a shared quad with a screen; offices first, never the cheapest tier; as `commercial` it dresses the district's industry as research labs and fenced server halls, and the assembly halls there are server halls; [r4-patchwork] `patchwork` is the Riverbank Shantytown story's look: corrugated-iron shacks grown room by room, stilt houses (on piles by the water), self-built storeys on an old concrete base with water tanks, a tarp market, corner kiosks and a shebeen (a mechanic or bike shop works under a lean-to there, a tow yard is a scrap yard; [r4-skyline] `skyline_luxe` is Neo Tokyo Heights' look: slim glass residence towers on podiums of noisy shops, twin towers on a sky bridge, rooftop pool towers, hotel and office towers with a helipad (a hotel there is one), a vertical mall in screens and a boutique row under neon, a sky villa for its mansions; it keeps the Neon strip's street (gadget shops and bakeries first, visitors shop there, a landmark) and the Eco homes' land lift, rises a floor over the block and builds a tier up; no industry), and as `commercial` it dresses the district's industry as the scrap yard and lean-to workshops; its homes are cheap and rise over the block's height as the Kowloon homes it replaces did, its shops and works keep to it), `industry` farming / oil / refinery / manufacturing / logistics / fishing / tech / none, `historic` bool, `height_limit` (m; null or 0 lifts it), `rename` | `id`, `name`, `policy`, `story`, `policies` |
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
{"cmd": "district", "action": "set", "district": "Lantern Row", "story": "afterglow_mile", "nightlife_levy": 0.12}
```

[stories] A story sets the building types; the policy dials and the height limit are the player's
to fine-tune. Every policy's money moves through the ledger: levies as the treasury's
`district levies`, support, grants and relief as `district spending`, itemised "District: policy".

`tourism {action?, limit?}` (also `tourists`): `attractiveness`, `demand_per_day`, `ways_in` (road,
rail, ferry, air), `tourists` (in town), `visible` (out on the street), `hotel_demand`, `today`
{arrivals, arrivals_by_mode, departures, turned_away, nights, sights_visited, spend, spend_hotels,
spend_shops, spend_venues, tourist_tax}, `yesterday`, `totals`, `ledger_tourist_tax`, `hotels` (id,
name, tier, beds, guests, money, revenue_today, district, pos), `attractions` (kind, name, score, pos,
building; strongest first) and `tourist_list` (`limit`, default 20: id, pos, activity, visible, class,
via, hotel, money, spent, sights, arrived_day, leaves_day, going_to). `action: "refresh"` re-reads
the attractions now instead of on the two-hour cycle. `status` carries a `tourism` summary and the
number of `districts`; the ledger has a `tourist tax` row.

[ferries] A tourist row also has `commuter_at` (a commuter from over the water: the business they
work a shift at; null for a visitor), and the reply `ferry_commuters` (commuters in town now; they are
not counted in `tourists`). The `landscape` reply's `ports` rows carry `remote_towns` ({name, edge
north/east/south/west, km}: the towns off the map a ferry terminal runs to, beyond edges its water
reaches), `sailings` and `waiting_to_board`; `ships` rows `remote_town` (a run off the map) and
`back_home` (unloading at its own pier); `port_stats` adds `remote_sailings_today`,
`visitors_in_today`, `visitors_out_today`, `commuters_in_today`, `commuters_out_today`,
`day_trips_today`, `day_trippers_back_today`, `abroad_now` and `commuters_in_town`. A citizen away
on a day trip has activity `away on a day trip` with `terminal`, `town` and `back_after_tick`.
Placing a ferry terminal on water that does not reach the map edge is refused.

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
"trash_depot"}` (also `trash`, `garbage`) puts down a depot (any lot or unzoned ground, a street out
front). Garbage trucks are cars in `find {what: "cars"}` with a garbage truck body.

## Business pressure, the waterfront and the art scene

`report {sections: ["pressure"]}` (also the `pressure` command with no fields): `pressures` (every
pressure, strongest first: `kind` (tow_company, mechanic, car_dealer, trash_company, bike_shop,
slaughterhouse, fish_gutting, fish_stick_factory, fish_market, seafood_restaurant, golf_range,
art_studio, gallery, marina, and [waterfront] beach_resort, pier_restaurant, beach_bar, boat_rental), `label`, `business` (the business kind key it opens), `strength`
0..1, `wanted`, `have` (open and being built), `gap`, `struggling`, `wants_more`, `why` (the
cause in words), `held` (why nothing opens despite the gap: cooling down after one closed, no
lot, a player-placed kind; with no lot it says the lot it needs and how the vacant lots of its land
uses fall short, e.g. "no vacant lot that suits it: it needs a vacant industrial lot of 500 m² or more
on a street within 1.4 km of a fishing port (vacant industrial lots: 3 too small, 12 too far)"),
`needs` [r4-hunger] (the lot its business needs, null for player-placed kinds: `text`, `uses`,
`min_area_m2`, `street`, `waterfront`, `near` (what the anchors are), `anchors` [[x, y]...],
`reach_m` (null: anywhere, best near the anchors)), `player_placed`), `opened_total` (by kind), `signals` (the smoothed
causes: outside and local tows a day, crashes a day, cars, rich households, cars bought a day and
of them out of town, buildings out of the city's trash rounds, overflowing bins, depots, bike
owners, bikes bought, long walks a day, livestock, fish and fillets a day, fishing ports and
marinas [small, large], comfortable adults, artists, artists of middle income, rich adults, rich
households near the water, has_water, art_scene; [waterfront] tourists, beach_visits, beaches_open,
beach_appeal, waterfront_sites), `trash` {local_companies, local_clients,
local_fee, out_of_town_fee}, `art` {openings_total, donations_total, art_sold_total, openings (the
last six: gallery, name, from/until tick, on_now, invited, visitors, donations, sales),
galleries_wanting_to_expand}, `waterfront` {boats_out, berth_fees_last_night, docks: kind, label,
building, pos, works, berths, boats_out, open}, `repairs_today`, `bike_repairs_today`.

`pressure {open, x, y}` (or `at`): opens a business of that kind (a `BusinessKind` key, label or
variant name: `mechanic` (`car_repair_shop`), `car_dealer` (`car_dealership`), `trash_company`,
`bike_shop`, `cow_farm` (`cattle_farm`), `chicken_farm`, `slaughterhouse`, `strip_mine`,
`fish_gutting` (`fish_gutting_shed`), `fish_stick_factory`, `fish_market`, `seafood_restaurant`,
 `beach_resort`,
`pier_restaurant`, `beach_bar`, `boat_rental` (these only on a lot that runs down to the water), and the older
kinds) on the vacant lot under or nearest the point (within 150 m), as a pressure would; `finish:
true` builds it at once. Reply `building`, `kind`, `lot`, `finished`, `story`. Marinas and fishing
ports are docks: use `place`.

`pressure_where {kind}` (alias `show_where`) [r4-hunger]: the Economy pane's "show where" for a
pressure: the map shows a post at each anchor with the reach drawn round it and outlines the
vacant lots of its land uses by fit (green suits, amber too small, orange on a footpath, blue not
on the water, grey too far), with a banner saying what it needs. Reply `on`, `kind`, `label`,
`held`, `needs` (as above), `lots_by_fit`, `lots` (lot, fit: suits | too_small | no_street |
not_on_water | no_jetty_room | too_far | taken, center, area_m2, land_use). `{off: true}` hides
it; with neither field, what is shown now.

`place {kind}` takes the docks `marina` (`marina_small`), `marina_large` (`yacht_club`),
`fishing_port` (`fishing_port_small`, `fish_quay`) and `fishing_port_large` (`fishing_harbour`) at
the water's edge, like `shipping_port` and `ferry` (refused away from the shore: "must be built at
the water's edge"). They are in the Build window under Water.

A building's `inspect` (and `selected`) has `story`: `why` (why the business is here), `doing`
(how it is doing: paying its way, struggling, days in debt, short-staffed, out of an input, more
of its kind than the city needs, competitors, donations, an opening tonight), `struggling`,
`wants_expand` (a thriving gallery: the hook for growing into a museum), `opened_by_pressure` (the
pressure key when its kind is one a pressure opens), `donations`, `opening_now` {invited,
visitors, donations}. Marina boats are port ships of kinds `Yacht` and `Sailboat` besides the
freighters and ferries; trawlers are the fishing fleets below.

### Fishing fleets [fishing]

A fishing port keeps one trawler per berth on its quay. On a working day each crewed boat
(boats out = berths × staffing, one day in eight a boat stays in for repairs) casts off in the
early morning (staggered 03:00–08:00), steams out (4.5 m/s) to one of the port's fishing grounds,
trawls there with its nets out (1.5 m/s sweeps, stopping every 50 min to haul for 12 min) and heads
home to tie up between 12:00 and 17:00, unloading for 40 min. A boat at sea leaves its berth empty
(the renderer draws every trawler where the sim has it). The catch is landed into the port's fish
stock when the boat ties up: per minute fished, times the ground's richness (further out is
richer), less up to 70% on water fouled by dirty industry (ground contamination reaching the water
and dirty plants nearby); a port's days add up to about what its recipe made by the hour before.
A storm (weather spell) keeps the fleet in and calls boats at sea home. A port with no grounds in
reach (cramped water) makes its fish by the hour as before. Fleets are saved; old saves start with
every boat in port.

Grounds are found per port from the landscape seed: open water at least 32 m from the shore,
300–2600 m away by water (routes on a 20 m water grid, never over land), at least 140 m from
shipping lanes and ferry routes and 200 m from other docks, up to five, 320 m apart. They show as
rings on the Water survey layer (`overlay {layer: "water"}`).

`inspect {building}` on a fishing port has `fishing`: `boats_total`, `at_sea`, `in_port`,
`landed_today`, `landed_yesterday`, `trips_today`, `kept_in` (why the boats stayed in today, e.g. a
storm), `fishes_grounds`, `offing` (where boats gather off the quay head), `boats` ([{name, state
(in_port, outbound, fishing, hauling, returning, unloading), doing, at, at_sea, nets_out, ground
(1-based), fish_aboard, minutes_fished, catch_today, home_by ("HH:MM" while out)}]), `berths`
([{at, boat, empty}]), `grounds` ([{ground, center, radius_m, richness, route_m, km_out,
fouled}]). [sweep-visual] A fishery with its own fishing jetty has the same (one trawler, sailing
from the jetty). Null for other buildings.

`ships`: every ship and boat on the water: `count`, `trawlers_at_sea`, `ships` ([{kind (freighter,
ferry, yacht, sailboat, fishingboat), port, at, heading_deg, docked} — trawlers also name,
port_name, state, at_sea, nets_out, fish_aboard, catch_today, and are listed at their berths too]).
`landscape` adds `trawlers` (those at sea: name, port, at, state). The pressure report's
`waterfront.boats_out` counts trawlers at sea.

### Waterfront lots, public beaches and private jetties [waterfront]

Where a street or path runs within 80 m of a river, lake or the sea, the land between it and the
water is parcelled the whole way down to the waterline: those lots run from the road to the shore
(new cities; a save from before keeps the parcels it was cut with, see `waterfront_parcels`).
Every lot knows its edge on the water: lot rows (`find`, `list`) carry `waterfront`,
`water_frontage_m` and `beach`; parcel rows `water_frontage_m`; a lot's `inspect` has `shore`
{waterfront, frontage_m, seaward, beach, beach_state}. Waterfront lots carry a land value premium
by their frontage; open beaches lift the land around them.

| Command | Fields | Reply |
|---|---|---|
| `shore` (alias `waterfront`) | | `parcels_reach_water`, `waterfront_lots`, `waterfront_vacant`, `waterfront_frontage_m`, `beaches`, `beaches_open`, `bathers_now`, `jetties`, `trades` {beach_resorts, pier_restaurants, beach_bars, boat_rentals, waterfront_hotels}, `pressures` (the waterfront trades' readings: kind, wanted, have, gap, why, held) |
| `beaches` | | `beaches` [{building, name, pos, area_m2, access, open, usable, note (no access, closed, too cold, a storm, ...), visitors, swimmers, people_here (the sim's own citizens and tourists on the sand), draw (the hour's crowd before rounding), capacity, crowded, catchment (residents within reach), appeal (weather and hour, 0..1), contamination, peak_today, visits_today, visits_yesterday, towel_spots}], `bathers_now`, `appeal_now`, `appeal_season` |
| `jetties` | | `jetties` [{building, name, business, kind (fishing jetty, cargo quay, restaurant pier, boat pontoon), root, tip, berth, water, berths, built_day, loads_landed, loads_coming}] |
| `waterfront_parcels` | `on` (bool; omitted reads) | `on`, `parcels_on_water`, `note`. Re-cuts the parcels now; lots follow at the next implement, and a building whose parcel changes shape is pulled down |

A **public beach** is a park sub-zone: `paint {land_use: "beach", ...}` (or the Zone tool after
Build > Parks > Public beach) on parcels that run down to the water (others are refused: "a beach
must run down to the water"); painting plain `park` over it makes it a park again. Its lot is laid
out as sand with a boardwalk and towel spots (the park's benches: people walk to one and lie down)
and gets a park building named for the beach. It joins the walk graph only through a street or path
it fronts, or a park next to it (never across a private lot); without either it stays empty with
the note "no access: connect a path or a road, or a park next to it". The crowd follows the
weather and the hour (`shore::beach_weather`: nobody under 12 °C, in a storm or at night, few in
the rain; Arctic beaches stay deserted) and the residents within 1.5 km; past its capacity it is
crowded. Contamination over the sand or the water off it at 20% closes it (a red sign and tape)
until it falls under 12%. Open beaches are tourist sights (attraction kind `beach`), cheer the
households within 700 m at midnight, and bring beach bars (pressure `beach_bar`).

A building's `inspect` adds `beach` (the row above) for a beach's park building, `jetty` for a
business with a private jetty, and `shore` for any building on the water.

**Waterfront trades** open by pressure on vacant waterfront lots only: `beach_resort` (lodging: two
beds a floor more than a hotel's, 1.6× the night), `pier_restaurant` (a place to go out at 2.4×
a venue's tab; its deck over the water is its jetty), `beach_bar`, `boat_rental` (its pontoon is
its jetty). A hotel opening for want of beds prefers a waterfront lot and is drawn as a waterfront
hotel.

**Private jetties**: a fishing company or fish works (fishery, fish plant, gutting shed, fish-stick
factory, fish market) or a works that ships (container yard, refinery, smelter, chemical works,
mill, parts fab, assembly plant, cargo hub) on a waterfront lot builds a small jetty of its own for
600 when it has the money and the water off it is navigable (the sea, a lake, a river at least 18 m
wide; never a stream). A fishing jetty is a trawler's berth; a cargo quay lands the business's
imports by its own coaster at the sea price (5 hours on; `loads_coming`).

## Street races, raves and the roads

The code is `crates/nd_app/src/remote/nights.rs`; the model is in DESIGN.md (Street races,
Warehouse raves, Road wear and maintenance).

| Command | Fields | Reply |
| --- | --- | --- |
| `street_events` (or `nights`) | `limit` (rows of history and news, default 10), `fresh` (bool: weigh the warehouses now instead of the last 22:00 roll) | `race` (below, or null), `race_planned` (time), `race_crew` (locals with a fast car free now), `rave` (below), `rave_planned` {building, name, doors}, `rave_spots` (warehouses best first: building, name, pos, `pull` {youth, artists, art_scene, neighbours, culture, has_scene, night_chance, why}, last_rave_day), `history` (newest first: id, kind, place, pos, finish, started, ended, minutes, people, crowd, winner, police, arrests, fines, complaints, chase, culture), `news` (seq, tick, time, kind, headline, pos), `totals` {races, raves, busts, fines} |
| `race` | `at` [x,y] optional: meet at the empty lot or surface car park nearest it | `race` id and `detail`; refused with fewer than two free locals with a fast car, or no lot pair 450 m-2.4 km apart (550 m by road) |
| `rave` | `building` id, or `at` [x,y] (the old warehouse nearest it); neither: the one with the strongest scene | `rave` id and `detail`. Forcing one skips the scene check and the day-10 start (the roll at 22:00 does not) |
| `repair_road` | `road` (a segment id, or [x,y]: the piece a click there picks); default the selected road | [road-inspect] "Repair now": the piece is resurfaced at once for the crews' price (lane-metres x wear x 6), paid from the treasury and booked as road maintenance. `repaired`, `cost`, `treasury`, `road` (the card, below). Refused for ways the crews do not mend, a piece like new, or a treasury short of the cost |
| `roads` (or `road_condition`, `road_budget`) | `funding` 0..1.5 (sets the road maintenance level; 1 = what the network needs), `age_days` (dev: wear every street that many days at yesterday's traffic, the crews working each night at the funding level unless `crews: false`; nothing charged), `near` [x,y], `limit` (default 10) | `pieces`, `lane_km`, `mean_wear`, `worn` (0.5+), `crumbling` (0.8+), `waiting_for_repair`, `mean_wear_by_land_value` {cheap, middle, dear} (tier cuts of land value), `budget` {funding, max_funding, need_per_day, allowance_per_day, spent_yesterday, repaired_yesterday, repaired_land_value_yesterday, ledger_today, ledger_yesterday, repaired_total}, `days` (the last 7: allowance, spent, repaired, repaired_lane_m, repaired_land_value, waiting), `pieces_listed` (worst first, or nearest `near`: segment, pos and the `condition` fields of `inspect {segment}`) |

A race `detail`: id, `stage` (gathering / racing / party), `meet` and `finish` {name, pos, lot},
started, go, ends, `route_m`, `racers` (car, driver, name, pos, speed_kmh, parked_in_lot, place,
phase), finished, crowd, incident, arrests, fined, fines, chase_incident. A rave: id, building,
place, pos, door, started, ends, promoter, promoter_name, guests, queue, queue_at_door, inside,
peak, complaints, incident, takings (on the door), pull.

`report.traffic` carries `street_events` {race, rave, races_total, raves_total, busts_total,
last_news} and `road_condition` (the `roads` reply without the list). The ledger has two more flows,
`road maintenance` and `fines`. `overlay {layer: "road_condition"}` shows the Road condition layer.
**Inspecting a road.** [road-inspect] A click with the Inspect tool on a road (not on a vehicle,
person, toll booth or parked car, and not inside a junction) selects the street piece under the
pointer: at a crossing of two levels the upper one, and in the Underground view only pieces below
ground. In Edit mode, clicking a road to upgrade it selects the piece clicked too, and a compact card
opens beside the ROAD UPGRADE panel. The piece is outlined in gold along both edges; the rest of its
run between junctions is traced faintly. `select {road: id}` (or `segment`, or `road: [x, y]`, or
`select {at}`) selects one; `inspect {road: id | [x, y]}`, `inspect {segment}` or `inspect` / `selected`
with a road selected returns the `segment` record plus `road`, the card:

- `name` (the street's), `road_kind` (street, avenue, highway, ramp, roundabout, path, rail), `level`
  (at grade, embankment, cutting, bridge, viaduct, tunnel), `lanes_fwd`, `lanes_bwd`, `one_way`,
  `speed_limit_kmh`, `length_m`, `from`, `to`, `run` {pieces, index, length_m} (the pieces between
  the junctions at either end; the figures are this piece's);
- `now`: `vehicles`, `cars`, `lorries`, `buses`, `emergency_or_city`, `bikes` and `walkers` (on it and
  its pavements), `mean_speed_kmh`, `allowed_kmh` (the limit less a rough surface's slowdown),
  `congestion` (empty, free-flowing, busy, slow, jammed), `queued`, `queue_m`, `per_lane_km`;
- `flow` {this_hour, last_hour, today, yesterday, peak {day, hour, vehicles}}: vehicles that drove
  onto the piece, both ways; `hourly`: the last 72 hours, oldest first (the last is the hour under
  way), from `hourly_from` {day, hour, before_start}. Kept per piece as 72 `u16` (made on the first
  vehicle) and saved with the city; old saves start empty;
- `decay` (null for ways the city's crews do not mend): `wear`, `condition`, `speed_factor`,
  `crash_factor`, `wear_per_day` (`wear_per_day_measured`: over its last whole day, else reckoned
  from its age and yesterday's traffic), `next_condition` {label, days}, `repair_cost`, `land_value`,
  `queued_for_repair`, `queue_place`, `queue_length`, `priority`, `last_repaired_day`,
  `last_repair_whole`, `last_repair_ordered`, `funding`;
- `tolls` [{id, name, price}], `crossings` {junction, zebra, push_button, footbridge}, `bus_stops`
  (line names), `kerb_parking`, `kerb_parked`.

```json
{"cmd": "select", "at": [120, -40]}
{"cmd": "inspect", "road": 57}
{"cmd": "repair_road", "road": 57}
```

Racer cars show `takeover {phase: "racing", ...}` in `inspect {car}` (the gang role they share with
takeover crews); crimes of kind `street race` and `illegal rave` appear in `incidents`.

```sh
curl -d '{"cmd":"race"}' http://127.0.0.1:4817/cmd
curl -d '{"cmd":"street_events","limit":5}' http://127.0.0.1:4817/cmd
curl -d '{"cmd":"roads","funding":0.3,"age_days":120}' http://127.0.0.1:4817/cmd
```

## People talking: friends, feuds and groups [social]

`social {days?, talks?, group?}`: the city's social life (DESIGN.md, Social layer).

- `days` (default 7): daily history to include; `talks` (default 10): the latest conversations.
- `group`: a group id; the reply is that group alone, `{group}`, with `member_list` (id, name,
  founder).

Reply:

- `today`, `yesterday`, `history`: day, talks, good_talks, bad_talks, friendships,
  close_friendships, couples, feuds, fights, meetups, home_visits, favours, loans, jobs_found,
  homes_found, moved_in, enemy_avoidance (place choices an enemy put people off), gossip,
  groups_formed, groups_folded, splits, works.
- `pairs` by bond city-wide (acquaintance, friend, close_friend, partner, rival, enemy);
  `people_with_ties`, `people_with_a_friend`, `population`; `totals` (talks, friendships, feuds,
  fights, couples).
- `groups` (active): id, name, kind (band, sports_club, book_club, street_art, startup, garden,
  neighbourhood), members, founder, founded_day, momentum, threshold (momentum it takes to make
  something), reputation, meetings, base and base_id (where it meets), works ({kind: gig | match |
  reading | mural | garden | business | petition | protest, day, ...}), split_from, folded_day.
  `folded_groups` counts the ones that folded.
- `petitions` (newest first): id, group, day, ask (more_police, lower_rent, crossing,
  clean_streets, park, jobs, quiet), concern, text, at, signatures, state (open, answered,
  protested, lapsed), rally (where they march).
- `murals` (group, building, at, quality), `gardens` (group, lot, at), `gigs` (group, venue, from,
  until, on_now, crowd), `outings_planned`, `walking_together` (pairs).
- `news` and `recent_talks` (when, a, b, a_id, b_id, topic, topic_label, outcome: agreed, laughed,
  flirted, favour, argued, insulted, fight; where: venue, stadium, bench, park, work, home,
  transit, stop, street; place, at, line).

`inspect {citizen}` has a `social` block: `ties` (id, name, bond: acquaintance, friend, close
friend, partner, rival, enemy; affinity, familiarity, trust, talks, met, met_at, since, last,
partner, lent), `groups` (id, name, kind, founder), `talks` (their recent conversations: when,
with, with_id, topic, outcome, place, where) and `walking_with`.

`window {name: "people", tab: "social"}` shows the Social tab in game. Speech bubbles show over
people talking (and at the door of a bar where they talk inside) when the camera is within about
320 m; a band's gig shows a note over the venue and a protest a fist over the rally point.

```sh
curl -d '{"cmd":"social","days":3,"talks":20}' http://127.0.0.1:4817/cmd
curl -d '{"cmd":"social","group":4}' http://127.0.0.1:4817/cmd
```

## The mayor's committee [committee]

The AI committee (docs/AI_COMMITTEE.md): fictional politicians, six seats, alerts between
meetings, a meeting every 30 sim days, votes, and projects for the mayor. The committee never
changes the city by itself: only `committee {action: "implement"}` (or the window's Implement
button) does.

`committee {action?}` (default `state`):

- `state`: `day`, `next_meeting_day`, `meetings_held`, `auto_meetings`, `veto_allowed`,
  `in_session` (number, stage, waiting_for_mayor) or null; `seats` (seat 1..6: id, name, scope,
  title, temper, party, age, leanings with spending/growth/order -1..1, goals, background,
  `record` {seated_since, times_appointed, times_dismissed, meetings, alerts, proposed,
  proposals_passed, votes_yes, votes_no, abstained}, `memory` lines; or `{seat, empty: true}`);
  `candidates` (the rest of the pool, brief); the last 10 `alerts`; `unseen_alerts`; the last 10
  `projects`; `backend` (choice mock|local|claude, speaking_through, error, local_url,
  local_model, claude_model, claude_effort, claude_fallbacks, claude_key (where the key comes
  from, never the key), timeout_s, max_tokens, calls, fallbacks, errors, test).
- `members` (alias `pool`): every politician in full, with `seat` or null.
- `appoint {id, seat?}`: to the first empty seat, or to `seat` (1..6), dismissing its holder.
  `dismiss {id}`: the seat stays empty until filled.
- `alerts {limit?, mark_seen?}`: newest first: id, day, member, name, rule, when (the rule in
  words), severity (1 notice, 2 warning, 3 urgent), text, phrased (the backend's words rather than
  the rule's), seen, facts; plus `rules` (key, scope, when, cooldown_days). `check_alerts` runs
  today's rules now (normally once a sim day) and returns `raised`.
- `projects`: every committee project: id, name ("Committee: Northside clinic"), meeting, proposal,
  by, status (pending, needs_mayor, implemented, dismissed, failed) and status_detail, action,
  pick (1-based), reason (the secretary's), world_project (zoning: the parked world project that
  previews it), options (option, label, point, parcels, stops, radius, area, area_m2, cost, score,
  notes), log.
- `implement {project}`: carry it out (build on the picked site, zone the parcels, add the bus
  line, set the tax, toll or road funding, dig out the ground); returns `done`. Fails (and the
  project stays open as `failed`) if the game refuses, e.g. the site was taken meanwhile.
  `dismiss_project {project}`, `pick_site {project, option}`, `refresh_sites {project}` (look
  for valid sites again).
- `report`: the compact city report the committee reads, as `text` (what a model is sent) and
  `report` (the figures).
- `ask {text}` (alias field `question`): [ask-committee] put a question to the seated members,
  as the Committee window's Ask tab does. Each member asked gets one turn (one at a time, in an
  order that rotates per question, reading the city report, their persona and up to 4 earlier
  replies in the thread) and decides whether to answer: a model replies `{"respond": bool,
  "text": "..."}`; the stand-in answers from templates and the real figures and sometimes
  declines. With the stand-in the whole thread is answered before the reply; with a model the
  answers arrive on later frames (read `questions` again). Reply: `asked` (the question's id),
  `backend`, `question` (as below). Fails with no members seated or an empty text; the text is
  cut to 400 characters.
- `questions {id?, limit?}`: the question threads, newest first (default 20; `id` for one):
  `id`, `day`, `text`, `discussed` (the number of the meeting that read it, or null), `complete`,
  `asked` [{id, name}], `waiting` (member ids still to answer), `typing` (being asked now),
  `replies` [{member, name, scope, respond, text, stand_in}] (`respond: false` is "no comment";
  `stand_in`: the model failed and the stand-in spoke). Also `kept` (the last 40 threads are
  kept, saved with the city), `undiscussed`, `answered` (replies filed this run) and
  `next_meeting_digest`: the text the next meeting will read. When a meeting opens, every
  answered thread not yet discussed goes into a digest (the newest 5, answers clipped, at most
  about 1,800 characters) appended to the city report in every turn of that meeting, the
  secretary notes it in the transcript, and the threads are marked discussed (they stay in the
  log). A thread still being answered waits for the meeting after.
- `minutes {meeting?}`: a past meeting (default the last), as `committee_meeting` returns one;
  `kept` lists the numbers kept (the last 6).
- `settings {backend?, local_url?, local_model?, claude_model?, claude_effort?,
  claude_fallbacks?, timeout_s?, auto_meetings?, veto_allowed?, save?, test?}`: switch the
  backend (applied at once; `save: true` writes `committee_settings.ron`), start a "Test
  connection" (`test: true`, ignored while one runs; the result shows in `backend.test` on a later
  call). Never takes or returns an API key: set `ANTHROPIC_API_KEY`, or paste one in the window.
  The backend fields (`backend`, `local_url`, `local_model`, `claude_model`, `claude_effort`,
  `claude_fallbacks`, `timeout_s`) are refused unless the player has ticked "Let the remote API
  change the backend" in the Committee window's settings (this run only; the reply's
  `remote_may_switch` says): a model may cost money and a URL points the committee at a server.

`committee_meeting {start?, vote?, abstain_rest?, fast_forward?, transcript_from?}` (alias
`meeting`):

- `start: true` calls a meeting now (the sim pauses until it closes).
- `vote: {proposal, vote}` with vote `yes|no|abstain|veto` (or `vote: "yes", proposal: n`): the
  mayor's vote. `abstain_rest: true` abstains on every proposal not yet voted on. The meeting
  waits at "the mayor's vote" until every proposal has one.
- `fast_forward: true` (alias `skip`): the window shows everything at once; with the stand-in the
  meeting runs to the mayor's vote (or the end) before the reply. A model keeps its own pace.
- Reply: `in_session`, and `meeting` (number, day, stage, on_demand, calls, waiting_for_mayor,
  done, members, proposers, `questions_digest` (the mayor's questions the meeting read, as sent
  to the models; empty if none) and `questions_read` (their ids), `transcript` from `transcript_from` (speaker: "secretary", "mayor" or
  a member id; name, kind: speech|agenda|vote|result|note, text), `proposals` (id, title, by,
  by_name, action, rationale, cost, votes [{member, name, vote, reason}], mayor, tally {yes, no,
  abstain}, outcome, would_be, project)); between meetings `next_meeting_day` and `last` (the
  last minutes).

`window {name: "committee", tab?: "meeting|ask|members|alerts|projects|settings"}` opens the window
(`ask`, alias `questions`/`chat`: the question chat room).

[voices] Through the same backend:

- `diary {citizen, refresh?}`: the citizen's diary of today, written from their memories and
  conversations: `day`, `text`, `pending` (a model still writing: call again), `by_model`,
  `backend`. The stand-in answers at once. Display only, not in the save.
- `voices {limit?}`: conversations of spotlit or favourite citizens that got a model's line
  (`serial`, `a`, `b`, names, `topic`, `template` (the sim's line), `line` (the model's)), and
  `lines_written`, `diaries_written`. With the stand-in no lines are asked (the template is the
  line).

```sh
curl -d '{"cmd":"committee"}' http://127.0.0.1:4817/cmd
curl -d '{"cmd":"committee","action":"appoint","id":9,"seat":3}' http://127.0.0.1:4817/cmd
curl -d '{"cmd":"committee_meeting","start":true,"fast_forward":true}' http://127.0.0.1:4817/cmd
curl -d '{"cmd":"committee_meeting","vote":{"proposal":1,"vote":"yes"}}' http://127.0.0.1:4817/cmd
curl -d '{"cmd":"committee_meeting","abstain_rest":true,"fast_forward":true}' http://127.0.0.1:4817/cmd
curl -d '{"cmd":"committee","action":"implement","project":21}' http://127.0.0.1:4817/cmd
curl -d '{"cmd":"committee","action":"ask","text":"Why are rents so high?"}' http://127.0.0.1:4817/cmd
curl -d '{"cmd":"committee","action":"questions","limit":5}' http://127.0.0.1:4817/cmd
```
