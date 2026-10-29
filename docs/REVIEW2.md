# Code review 2: HEIFUNG://CITY (then Neon District) at commit 7f44653 (October 2026)

**Scope:** the whole workspace, read-only, after the car-life, driving, services, society, places, elevation, roundabout, ramp, remote API and lorry-unloading work.

**How it was done:**
- Five parallel reviews (sim core and save; traffic and roads; services and society; procgen; app and remote) fed this report.
- Every finding below was checked against the code by the consolidating reviewer.
- 18 findings were reproduced by probes. The probe files are on this branch, in `crates/nd_sim/tests/probe_*.rs` and `crates/nd_procgen/tests/probe_procgen_*.rs`.
- The probes assert the bug is present, so they **fail** while the bug exists. Turn them into regression tests as each fix lands.
- Anything that could not be substantiated was dropped.

**Baseline:** `cargo test --release -p nd_sim -p nd_procgen -p nd_minds`: all green (nd_sim lib 298 passed, 3 ignored; `demo_city` 3, `society_demo` 4).

The legend is P = reproduced by a probe (probe name given), R = verified by reading the code path.

---

## 1. What is solid

- **Determinism holds:**
  - Two 3-day demo cities are identical, and 300 ticks from the autosave run twice are byte-identical.
  - Every `HashMap`/`HashSet` in `nd_sim` (claims, `segment_lanes`, ring nodes, `by_car`, `coverage`, `walkers`, …) is used for lookups or order-free sums only.
  - Sub-RNGs are reseeded from (seed, tick).
  - `Instant::now` appears only in tests.
- **Save compatibility:**
  - Saves from commits 56d4960, 50272a1 and e666bf5 and today's autosave all load and run a day.
  - Every field added since 5 Oct has `#[serde(default)]` or `skip`.
  - Only saves from the first day (4 Oct) fail.
- **Robustness:**
  - A fuzz run (road works through built blocks, forced demolitions and constructions, mid-run save/load, unzoning) produced no panics and no lost citizens.
  - A procgen sweep of 76,230 `generate_styled` runs over sliver, acute and curved lots produced 0 panics, 0 NaN and 0 bad indices.
  - The vehicle catalogue and ships are finite, with no flipped faces.
  - Indices are `u32` end to end.
- **Remote API input handling:**
  - Ids are validated (`key_from` + `get`).
  - Grids, limits and steps are capped.
  - UI panes check `contains_key` before indexing.
  - Favourites are pruned.
- **Lorry unloading:** a lorry only leaves the road at a lot, at its own yard, or at the map edge.
- **Takeovers:** `take_over_lot`, `build_power` and `place_port` refuse to replace a civic structure.

---

## 2. Findings, ranked

### CRITICAL

**C1. The tick panics when a station is removed while one of its units is on a job.** P `probe_society_units::probe_ambulance_whose_hospital_burns_down_while_on_scene`

- **Where:** `nd_sim/src/world/services.rs:616-620`. When a station is missing, its units and their cars are deleted, but `Incident.unit` and the incident's stage are left alone.
- **Panic path:** `finish_scene` (`:1531-1535`) → `drive_unit` indexes `self.services.units[uid]` (`:1470`) and panics with `invalid SlotMap key used`.
- **Scenario:**
  1. An ambulance is on scene.
  2. Its hospital burns down (H2 makes this routine) or is bulldozed.
  3. Another hospital with a free bed exists.
  4. The scene work finishes and the game crashes.
- **Fix:**
  - When retiring a station's units, re-queue their incidents: Dispatched or OnScene go back to `stage = Reported, unit = None, dispatch_at = Some(tick)`. Transporting incidents are delivered or resolved.
  - Make `drive_unit`, `deliver_patient` and `finish_scene` use `units.get`.

### HIGH

**H1. Any plan edit, even zoning far from every road, takes every city vehicle off the road.** P `probe_traffic_rebuild`

- **Where:** `world/plan.rs:792-800`. `rebuild_from_master` calls `traffic.rebuild` and then `clear_cars()`. Only cars that `is_through_car` and are within 8 m of a highway are resumed. Every other car goes to `strand_car`.
- **What happens to the stranded vehicles:**
  - Private cars are teleported into parking.
  - Lorries deliver instantly and vanish.
  - Buses vanish, and their riders are left standing.
  - Service and trash vehicles vanish and are "recovered" later.
- **Who calls the rebuild:** `implement()` (every road, zone or roundabout edit), `place_rail_facility` (`world/trains.rs:70`) and ports (`world/water.rs:666`), even when nothing needed fusing.
- **Probe:** zoning a 30x30 block at (-260, 600) → `cars on road before 135 (through 95), after zone edit 95 (through 95)`. All 40 city vehicles were gone.
- **Invariant broken:** "vehicles are never deleted except at the edge or into parking".
- **Fix:**
  - Skip `traffic.rebuild`/`clear_cars` when the `RoadNetwork` geometry is unchanged. Zone-only edits and fused placements don't change it.
  - Otherwise re-map each car to the nearest lane on its old position and replan, as `resume_through_car` already does.

**H2. Civic structures burn down and are removed.** P `probe_society_civic`

- **Where:** `start_fire` (`world/services.rs:338`) excludes only construction sites, parks and rail. The hourly roll (`:559`) excludes only monuments.
- **What happens:** `burn_down` → `demolish_with`, and `begin_demolition` refuses civic structures, so the building is removed immediately.
- **Probe:** `t+853: the power plant burned down and was removed`, `t+868: the hospital burned down and was removed`.
- **Consequence:** this is also the trigger for C1 and H3.
- **Fix:** have `start_fire` skip `is_civic_structure()`, or cap fire damage on civic buildings at derelict and repair them.

**H3. Incidents are orphaned forever when their unit or aircraft disappears.** P `probe_society_units::probe_dispatched_calls_orphaned_when_station_burns`

- **What happens:** after the removal in C1, `step_incident` has no branch for a Dispatched incident that was not sent from outside town, and none for Transporting. The incident stays active forever.
- **Probe:** `crime incident 16: stage Dispatched active true … unit exists false`, 2000 ticks after the police station burned.
- **Consequences:**
  - `start_crime` refuses that building forever (`:457`).
  - A Dispatched medical case keeps pushing the patient's `next_decision_tick` back every tick (`:1064-1072`), so the patient is pinned in place.
  - A crash incident never clears, so its wrecks are never towed (`driving.rs:292`) and the road stays blocked.
- **Same class (R):** `scrap_drone` (`world/air.rs:131-137`) resets only Dispatched and Transporting. An OnScene air-medic incident has `work_left = f32::MAX` (`:747`), and `f32::MAX - 1.0 == f32::MAX`, so the patient is pinned forever.
- **Fix:** the re-queue from C1, plus a sweep in `step_incident` that re-queues any active incident whose `unit` no longer exists.

**H4. The remote API can be driven by any web page, and it writes to arbitrary paths.** R

- **Where:** `nd_app/src/remote.rs:144-178`. `serve` checks only the method and path. It never looks at `Origin`, `Host` or `Content-Type`.
- **Scenario:**
  - A page in a browser without local-network gating sends a no-preflight request: `fetch("http://127.0.0.1:4817/cmd",{method:"POST",mode:"no-cors",body:'{"cmd":"save","path":"C:/Users/.../anything"}'})`.
  - `Sim::save` (`sim.rs:184-190`) runs `create_dir_all` and then overwrites that file.
  - `export` (`.json`) and `screenshot` (`.png`) also write to the caller's path.
  - `load` and `new_game` wipe the running city.
  - With DNS rebinding the attacker can also read the replies.
- **Fix:**
  - Reject any request with an `Origin` header.
  - Require `Content-Type: application/json` (`tools/mcp/nd_mcp.py:36` already sends it) and a `Host` of `127.0.0.1:<port>` or `localhost:<port>`.
  - Confine save, load, export and screenshot paths to their folders: no absolute paths, no `..`.

### MEDIUM

**M1. Vehicles are deleted mid-street on many fallback paths.** R (invariant)

| Where | What happens |
|---|---|
| `world/cars.rs:563` (`send_back`) | A van or truck with no base and no route is removed. |
| `world/enforcement.rs:116-119` | Patrol cars vanish when their office is demolished. |
| `world/enforcement.rs:130` | A patrol car with no route home is removed. |
| `world/enforcement.rs:407-413, 434` | A tow truck with no route vanishes, and the towed car is teleported to the pound. |
| `world/driving.rs:325-328` (`tow`) | The wreck is removed before a route is known. This contradicts the comment "Nothing is simply deleted". |
| `world/services.rs:615-620` | Station units vanish (see C1). |
| `world/buses.rs:110-117` and the `gone` closure | Buses vanish. |
| `trash.rs:555` (`truck_home`) | A trash truck vanishes at the kerb when its depot is gone. |
| `world/mod.rs:612-615` (`validate` on load) | Lorries and buses with a dead destination are deleted. |
| `world/mod.rs:810` | Any arrival no handler claims is removed "as through traffic at the far edge", wherever it is. |

- **Fix:** one `retire_vehicle(car)` helper that routes the car to the nearest edge exit or parking. If neither is reachable, the car holds (`pull_in`) and retries, as `lorry_arrived` already does.

**M2. Service units, tow trucks, trash trucks and takeover outsiders count as through traffic.** R

- **Where:** `world/water.rs:521`. `is_through_car` is true for any car with a default citizen, no cargo, no bus, no owner and `errand == Trip`.
- **Which vehicles match:** all of these are spawned with `spawn_car(CitizenId::default(), None, …)` and keep those defaults:
  - service units: `world/services.rs:1358`, `:1473`
  - wreck tow trucks: `driving.rs:328`
  - takeover outsiders: `driving.rs:555`
  - trash trucks: `trash.rs:509`
- **Effects:**
  - On a rebuild (H1), any of these within 8 m of a highway is re-spawned as an anonymous through car, while its owner also recovers it by teleport. The vehicle is duplicated.
  - They count toward `MAX_THROUGH_CARS` (`water.rs:540`), which suppresses real through traffic.
- **Fix:** add a dedicated `Errand::Through`, set it in `step_through_traffic` and `resume_through_car`, and test for that.

**M3. Every plan rebuild renames every `LotId`, but stored lot keys are not remapped.** P `probe_core_lotkeys`

- **Where:** `world/plan.rs:708`. `std::mem::replace(&mut self.lots, SlotMap::with_key())`.
- **Probe:** after a road, "of 506 old lot keys: 0 dangling, 0 same lot, 506 now name a DIFFERENT lot". Because none dangle, `.get()` guards never catch it.
- **Stale keys:**
  - `services.rubble` (`world/services.rs:1190`): a burned lot becomes buildable at once (`develop.rs:560`), while a random other lot is blocked.
  - `society.homeless.camps[].lot`
  - `Citizen.pending_footfall` (`trips.rs:1346`): footfall is credited to the wrong shops.
  - `services.coverage`: stale until the next hourly refresh.
- **Fix:** in `rebuild_from_master`:
  - Re-key rubble and camps by their stored position (`rubble_lot(at, ..)` exists).
  - Clear `pending_footfall`.
  - Call `refresh_coverage()`.

**M4. Async mind answers are applied to citizens who are mid-trip.** P `probe_core_async`

- **Where:** `world/mod.rs:748`. The answers from `minds.poll()` reach `apply_decision` with no `is_traveling()` check, unlike `step_citizens`.
- **Effect with `HybridMind`:** the citizen starts walking while their car drives on empty, or while they are still in the bus's `passengers` list.
- **Probe:** 128 injected answers gave a worst case of 4 bus riders off their bus and 14 cars driven by someone not in them (baseline 0, 0).
- **Fix:** defer answers for traveling citizens to their next decision, applying only the thought and memory now.

**M5. Civic buildings decay to derelict and are never repaired.** P `probe_society_civic`

- **Where:** `world/develop.rs:288-296`. Daily decay also applies to `special` buildings, but refit and repair skip them.
- **Measured rates:**
  - Low-tier police station: about 0.014/day net when full, so derelict in about 40–50 days.
  - Mid-tier hospital: about 140 days.
- **Effect once derelict:** `crewed_units` is 0 and coverage is 0, but upkeep is still charged and fire risk doubles.
- **Fix:** skip decay for `is_special()` buildings, or add maintenance paid from upkeep.

**M6. Docks are retiered and re-jobbed by the daily market update.** P `probe_society_port`

- **Cause:** `Building::is_special` (`buildings.rs:366`) omits `port`, while `is_civic_structure` includes it.
- **Probe:** `day 2: port tier Mid -> Low, jobs 30 -> 8`. A dock can also become eligible for redevelopment.
- **Fix:** add `|| self.port.is_some()` to `is_special`.

**M7. Removing the stop a bus is heading to teleports the bus.** P `probe_traffic_busstop`

- **Where:** `world/buses.rs:74`. The bus index is decremented on `next_stop >= idx`, but riders' indices use `> idx` (`:96-101`).
- **What happens:** the bus is re-aimed at the previous stop. `bus_arrived` then routes from `here.segment` and `reroute` jumps it.
- **Probe:** `bus jumped 68 m`.
- **Fix:**
  - Decrement only on `> idx`.
  - On `== idx`, keep the index (clamped or wrapped) and reroute now.
  - Route from the car's own lane.

**M8. Player lane arrows are keyed by raw `SegmentId`/`NodeId` and get lost or misapplied after a rebuild.** P `probe_traffic_laneuse`

- **Where:** `traffic.rs:906` (`LaneUseRule`) and `:1516-1523`.
- **Probe:** removing an unrelated earlier road renumbers the approach (node 830 → 828, seg 824 → 821). The rule still says 824/830, and `fixed` becomes false. A rule that lands on a reused id is applied without the `exits` check that `set_lane_use` (`:1985-1990`) performs.
- **Fix:** key rules by geometry (node position plus the far-end direction), re-resolve them after a rebuild, re-validate `arrows & exits`, and drop rules that no longer match.

**M9. Street height-lifting is O(vertices × run points): about 1 s per road edit when relief is on.** P `probe_procgen_street` (heights scaling)

- **Where:** `nd_procgen/src/street.rs:290-303` (`profile_height`), called per vertex via `finish_run` (`:345-351`).
- **Probe:** an 8x8 grid plus the 10 km outer road gave zeros 51 ms vs non-zero heights 877 ms; a 16x16 grid gave 101 ms vs 1.02 s.
- **Why it hits players:** nd_app passes `road_z` on every `roads.version` change when relief is on (`render.rs:872`), so every road edit stalls the frame for about a second.
- **Fix:** lift each piece from its own segment's endpoint heights, or use an arc-length lookup per run.

**M10. Street layers leak: surface furniture appears in the Underground mesh, and zebras appear inside tunnels.** P `probe_procgen_street`

- **Where:** the mid-block crossing loop (`street.rs:711-735`) and the ped-signal posts (`:739-750`) never call `inp.layer.keeps(..)`. `nd_sim::walk::midblock_crossings` does not exclude tunnels.
- **Probe:**
  - The Underground layer had 2,368 vertices on a surface street.
  - The Surface layer had 876 vertices along a tunnel.
- **Fix:** filter both loops by the segment or node layer.

**M11. `walk_detail` ignores elevation, tunnels and highways.** P `probe_procgen_street`

- **Where:** `nd_procgen/src/walk.rs:42-90` filters only on `kind != Street`, and `lift_spawned` doesn't lift `WalkDetailTag`.
- **Probe:** 8,633 detail vertices at y ≈ 0.1 over a tunnel at −12 m; viaduct items stay at street level.
- **Fix:** skip tunnel and motorway segments, and add `road_height` to each item.

**M12. `full_rebuild` is cleared before most of its readers run.** R

- **Where:** `nd_app/src/render.rs:1572`. `sync_buildings` clears the flag ("so every sync system saw it", which is false).
- **Systems that run after it:**
  - `emergency::sync_emergency`, `stunts::sync_stunts` and `turbines` (scheduled `.after(sync_cars)` / `.after(sync_trains)`). Their `if sim.full_rebuild { drain }` branches are dead code.
  - `layers`, `lights`, `far` and `sync_parked_cars`.
  - `AirState` and the bots and parked-car maps have no rebuild handling at all.
- **Scenario:** after Load, reused slotmap keys keep the old world's rubble, drone and parked-car meshes. This is the "old car mesh for a new bus" bug the previous review fixed for the other maps.
- **Fix:** clear the flag in a system ordered after all of `SimSet::Sync` (or in `PostUpdate`), and add drain-on-rebuild to air, bots and parked cars.

**M13. Held camera keys act while you type in a text field.** R

- **Where:** `nd_app/src/camera.rs:175-186` (walk) and `~294-317` (orbit). They use `keys.pressed` with no `keyboard_captured` check; only the one-shot `key()` helper (`:117`) has it.
- **Scenario:** typing "Westside" into the project-name field pans and rotates the camera.
- **Fix:** early-out the held-key blocks on `flags.keyboard_captured`.

**M14. Autosave blocks the main thread and is not atomic.** R

- **Where:** `nd_app/src/ui/saves.rs:201-210` and `sim.rs:184-190`. The whole world is serialised to RON on every `NewDay`, which is every 5–6 s at 1024x. `fs::write` truncates in place, so a crash mid-write corrupts the save.
- **Fix:** write to `.tmp` and then rename; serialise off-thread, or throttle autosave at high speed.

**M15. Remote request bodies are unbounded and served on a single thread.** R

- **Where:** `remote.rs:153-155`. `read_to_string` has no cap.
- **Scenario:** a client that sends a large `Content-Length` and trickles bytes stalls every later request.
- **Fix:** `take(1 MB)`, reject on `body_length()`, and set a read timeout.

### LOW

| # | Finding | Where | Evidence | Fix |
|---|---|---|---|---|
| L1 | Remote `add_road` lane counts: `as u8` truncates, and `lf + lb` is a `u8` add before the `> 4` check. `{"lanes_fwd":200,"lanes_bwd":100}` overflows, which panics in the dev profile and takes the game down. `257` silently becomes 1. | `nd_app/src/remote.rs:855-860` | R | Range-check the u64 before casting. |
| L2 | `reopen_after_rebuild` is unreachable: the `redevelop` branch `continue`s at `:315` before the call at `:340`. Redeveloped buildings keep their old, possibly broke, business. | `nd_sim/src/world/develop.rs:309-342` | R | Call it when the redevelopment finishes (`construction.rs`); delete the dead branch. |
| L3 | `HybridMind` `pending`/`in_flight` survive Load and New game, so an answer lands on a new citizen who reuses the key. `in_flight` has no TTL. | `nd_app/src/sim.rs:73-78, 192-196`; `nd_minds/src/lib.rs:387-472` | R | Add `HybridMind::reset()` on load and new game; add a request TTL. |
| L4 | UI Load does not clear `Follow` or reset `History`; remote load does both. | `ui/saves.rs:133-145` vs `remote.rs:375-386` | R | Share one load routine. |
| L5 | Mojibake (double-encoded UTF-8) in visible strings: "â˜… favourite" (`ui/person.rs:139`), quotes around thoughts (`:58`), "â€¦" in API output (`remote/report.rs:234`), and "Â±"/"Ã—" plus a BOM in `docs/REMOTE.md`. | listed | R (byte dump) | Re-encode as UTF-8. |
| L6 | `services.by_car` is `serde(skip)` and is rebuilt only at the end of `step_services`, so on the first tick after a load an arriving unit car is treated as through traffic and deleted. | `world/services.rs`, `world/mod.rs` `step_traffic` | R | Rebuild it in `after_load`. |
| L7 | A save/load round trip is not transparent, because walker state (`pedestrians.rs:138`) and the in-progress land-value builder (`world/mod.rs:217/308`) are not saved. Positions diverge 1 tick after a round trip. | listed | P `probe_core_roundtrip` | Serialise walkers (as a BTreeMap); restart the land-value cycle on load. |
| L8 | `demolish_with`'s immediate path clears `Citizen.home` but not `Household.home`, which dangles until the hourly `sync_households`. | `world/plan.rs:928` | P `probe_core_fuzz` | Clear `hh.home` as `vacate` (`construction.rs:399`) does. |
| L9 | A takeover stunt car with no destination or lane is skipped by `continue` and does donuts forever. | `world/driving.rs:779` | R | Fall through to the `tow`/`end_stunt` fallback. |
| L10 | Quad normals use corners 0, 1 and 3 only. Non-planar or concave quads flip: TwistTower had 772 of 21,680 triangles flipped (the floor band at `commercial.rs:313` lofts `prev` to a twisted `plate`). | `nd_procgen/src/mesh.rs:181-193` | P `probe_procgen_flips` | Use a per-triangle normal when the quad is non-planar; loft from `plate` to `plate`. |
| L11 | The ship deck is capped twice: `frustum` already caps `top`, and an extra `cap` z-fights. | `nd_procgen/src/terrain.rs:822-823` | P `probe_procgen_mesh` | Drop the extra cap. |
| L12 | `slab` with width 0 gives NaN (`normalize`). There are no current callers with 0. | `mesh.rs:522` | P `probe_procgen_mesh` | `normalize_or_zero` plus an early return. |
| L13 | Signal posts do two O(nodes) nearest scans each, and `.all()` over an empty segment list marks an orphan node "underground". | `street.rs:754, 773` | R | Use `node_near` once and require at least one alive segment. |
| L14 | Crane fallback: `.normalize_or_zero().max(Vec2::X)` is a component-wise max, not a fallback. | `nd_procgen/src/buildings/construction.rs:396` | R | `normalize_or(Vec2::X)`. |
| L15 | `assets::poll_assets` resets the change flags unordered against the renderers, so a hot-reload can be missed. `apply_window_requests` is unordered against `side_panel`, which contradicts REMOTE.md's "same frame". | `assets.rs:29`, `remote.rs:95` | R | Order them `.before(..)`. |
| L16 | A remote 504 does not cancel the queued command, so a client retrying `step` or `new_game` runs it twice. | `remote.rs:165-170` | R | Document it, or add a request id. |
| L17 | `Tool::Road if roundabout` and `Tool::Road if !roundabout` cover every Road case, so the Road half of the next `Road / Zone` arm in `draw_gizmos` is dead. | `nd_app/src/tools.rs` ~1210/1239 | R | Remove it or merge. |
| L18 | Dead code: `Gesture::set_level` (`plan.rs:205`), `let _ = kind;` (`world/market.rs`), `let _ = tick;` (`buses.rs`), `if self.detail` (`vehicles.rs:225`). | listed | R | Delete. |
| L19 | `FigureCache` eviction and lending follow `HashMap` order. This is visual-only nondeterminism. | `nd_procgen/src/people.rs:362-372` | R | Use a BTreeMap or a stable tiebreak. |

**REMOTE.md and `help` versus the code (R):**
- `speed` is documented as 0..4 (1x–256x) with "value = sim minutes per real second". The code has 6 speeds (up to 1024x), and the value is a multiple of real time (`sim.rs:18`).
- `parking_price` and `concerns` are undocumented.
- `help` lists only fire, crime and medical incidents; crash and takeover also exist.
- The `window` list omits `emergencies`.
- Events omit the incident, crash, takeover, arrest and stadium kinds.

### Performance hotspots (R, not measured)

These grow roughly with the square of population:
- `crime_pressure` (`crime.rs:304`): every adult scans every offender each hour.
- `hourly_homeless`: `Vec::contains` inside a loop over households, and `camp_spots` re-run per household.
- `choose_place` (`world/places.rs:122`): rebuilds `crowds()` over all citizens on every decision.
- `rate_visit` (`:327`): scans every citizen per visit.
- `board_waiting` (`world/trains.rs:511`): scans every citizen per stop arrival.
- `daily_trash` (`trash.rs:670`): overflowing buildings × all homes.
- **Fix:** build one per-tick or per-hour index (crowds per building, waiters per stop, an offender-home grid).

In nd_app:
- `sync_lots`, `sync_parks` and `sync_vacant` hash every lot every frame, even when paused (`render.rs:1190-1199, 1321-1326, 1375-1386`).
- `layers.rs:179-213` rebuilds overlay meshes on timers even when paused.
- `draw_parked` (`vehicles.rs:951`) builds an uncached full mesh per parked car.
- **Fix:** gate these on version counters.

---

## 3. Test gaps

No test covers:
- a station or office removed while its units are out (C1, H3, M1);
- civic buildings and fire or decay (H2, M5);
- docks through the daily update (M6);
- an unrelated plan edit leaving cars on the road (H1);
- vehicle classification (M2);
- `LotId`-keyed state across a rebuild (M3);
- async `poll()` answers during a trip (M4);
- lane-use rules across a rebuild (M8);
- removing the next stop of a bus (M7);
- save/load transparency mid-run (L7);
- the business after a redevelopment (L2);
- procgen layer separation and `walk_detail` on elevated or tunnel roads (M10, M11);
- `profile_height` cost (M9).

Most probes on this branch can become these tests once their asserts are flipped.

## 4. Unverified leads (not counted above)

- A "flat" world produced a node at (−5000, −480) with `road_z` 109.9. This was seen once during the procgen probes and not investigated.
- `strand_car` teleporting private cars into parking during H1 is arguably "into parking". It is listed under H1 because it happens to every car on every edit.

---

## 5. Status after the fix/review pass (7 October 2026)

M1–M3, M5, M6, M8–M11 and M13–M15 were fixed by earlier merged commits (see todo.md). The rest were checked against the code at c8d9a49:

| # | Status | Where / why |
|---|---|---|
| M4 | Already fixed | `World::tick` passes polled answers to `apply_late_decision` (`world/mod.rs`, `world/trips.rs`): only the thought and memory are taken mid-trip, and the intent waits up to `DEFERRED_INTENT_TTL`. Tests: `demo_city.rs` (M4 section), `sim_checks.rs`. |
| M7 | Already fixed | `stop_removed` (`world/buses.rs`) decrements only on `> idx`, keeps the index on `== idx` and reroutes from the bus's own lane. Test: `demo_city.rs` (M7 section). |
| M12 | Already fixed | `clear_full_rebuild` runs `.after(SimSet::Sync)` (`nd_app/src/render.rs`); air, bots and parked cars drain on a rebuild. |
| L1 | Already fixed | `add_road` range-checks lanes before the cast (`remote.rs`). |
| L2 | Already fixed | `reopen_after_rebuild` is called from the redevelopment path in `construction.rs` (when it starts, not when it finishes). |
| L3 | Already fixed | `HybridMind` has `REQUEST_TTL` and `reset()`; `city_replaced` makes a fresh mind on load and new game. |
| L4 | Already fixed | A city generation counter stops Follow and resets History after any load. |
| L5 | Fixed here | Visible strings were already clean (the inspector redesign rewrote the "Â·" lines). Six nd_procgen comments still had "â”€", "â€“", "Ã—"; re-encoded. `nd_sim/tests/source_encoding.rs` now fails on any double-encoded pair in `crates/**/*.rs`. |
| L6 | Already fixed | `after_load` rebuilds `services.by_car`. |
| L7 | Already fixed | Walkers are saved (a `BTreeMap`); the land-value work is serialised. |
| L8 | Already fixed | `demolish_with` clears `Household.home`. |
| L9 | Fixed here | `end_takeover` (`world/driving.rs`): a stunt car with no home, no exit or no lane there is towed off instead of skipped. Test: `a_stunt_car_with_nowhere_to_go_is_towed_not_left_spinning`. |
| L10 | Already fixed (mostly) | `mesh.rs` `quad` picks the diagonal by concavity and uses per-triangle normals on non-planar quads, so the flipped faces are gone. Won't fix: re-lofting TwistTower's floor band plate to plate (`commercial.rs`) changes the building's look for no visible gain now. |
| L11 | Already fixed | The ship deck is lifted 6 cm off the hull top. |
| L12 | Already fixed | `slab` returns early on zero width. |
| L13 | Already fixed | Signal posts use their own node; `tunnel_node` needs at least one live segment. |
| L14 | Already fixed | `normalize_or(Vec2::X)`. |
| L15 | Already fixed | `poll_assets.before(SimSet::Input)`, `apply_window_requests.before(side_panel)`. |
| L16 | Already fixed | A 504 withdraws a queued command; a running one is reported as "may still take effect". |
| L17 | Already fixed | `draw_gizmos` has separate Road and Zone arms. |
| L18 | Fixed here | Removed `Gesture::set_level`, `let _ = kind;`, two `let _ = tick;` (market, trains) and the redundant `if self.detail` in `vehicles.rs`. |
| L19 | Already fixed | `FigureCache` uses `BTreeMap`s. |
| REMOTE.md / help | Already fixed, one nit fixed here | Speeds, `parking_price`, concerns, incident kinds, the `window` list and events were all brought up to date. The `incidents` filter's help and doc comment now list `crash` too. |
