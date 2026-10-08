# nd_sim core review (everything outside `src/world/`)

Reviewed at `8e68a3e` (2026-10-08). Read-only review. Scope: `crates/nd_sim/src/**` except `world/`,
plus `crates/nd_sim/tests` and `crates/nd_sim/examples`. Paths below are relative to
`crates/nd_sim/src/` unless they start with `tests/`, `examples/` or another crate.

`cargo check -p nd_sim --all-targets --profile fast` is clean apart from one warning: an unused
`LaneKind` import at `world/ramp_tests.rs:6`. All 24 examples compile, and `perf_probe` compiles
with `--features profile`.

**A note on saves.** Saves are pretty RON (`World::to_ron`), which stores enum variants **by name**.
Inserting a variant mid-enum therefore does *not* break saves. Two things do:
- **(a)** fixed `[T; N]` arrays sized by an enum count, because RON rejects a tuple of the wrong length;
- **(b)** tuple-serialized structs.

These are the real save hazards, and they are listed in §1.3.

---

## Top 20 actions

### (a) Bug fixes

| # | Sev | Where | What |
|---|---|---|---|
| 1 | **High** | `tourism.rs:338` | `score_by_kind() -> [f32; 6]`, but `AttractionKind` has 7 variants (`Beach` = 6). It panics when a beach attraction exists. The UI (`nd_app/src/ui/districts.rs:206-211`) indexes `by_kind[6]` for every kind, so **drawing the tourism pane panics every time**. Return `[f32; AttractionKind::ALL.len()]`. |
| 2 | **High** | `traffic.rs:3513-3530` | A stale `idx` is used after the 30-minute replan. The snapshot `(lane_id, idx, len, s, s_end)` is taken before `replan_from_here` (which sets `idx = 0` and swaps the route, `traffic.rs:2423-2434`). The check `idx + 1 == route.len()` then compares the *old* idx against the *new* route. A stalled car can be snapped to `s_end` and marked arrived blocks from its goal, or skip lane advance for that substep. Fix: take the snapshot after the replan block, or `continue` after a successful replan. |
| 3 | Med | `traffic.rs:2035-2052` | A* has no closed set and no stale-entry skip, and duplicates count toward the 20,000-expansion cap. On large maps this gives false "no route" results, which then cause strandings and replans. Store `g` in `Open` and skip `popped_g > g[lane]` before counting. The same pattern is in `roads.rs:1188` (`path_where`) and `walk.rs:1268` (`route_nodes_with`). |
| 4 | Med | `road_wear.rs:200-262`, `road_traffic.rs:128-170` | Splitting a street loses its wear and history on *both* halves. Matching is midpoint to midpoint within `MATCH_REACH = 4.5 m`, so any piece longer than 18 m fails to match after a split. That contradicts the doc comment, and adding a junction resurfaces the street for free. Also `load[i]` is carried over by index even though segment ids are renumbered (`road_wear.rs:257-261`). Match each new midpoint *onto the old segment* instead (`project_on_segment` distance under ~1 m), share one matcher between the two modules, and add a split test. |
| 5 | Med | `services.rs:1482`, `society.rs:21`, `air.rs:409`, `concerns.rs:321` | RNG streams and dedup state marked `#[serde(skip)]` are lazily reseeded from `(seed, tick of first use)` after a load. A loaded game therefore diverges from one that kept running: fires, crime, illness, rats, drones, and complaint double-counting all change. Either save them with the existing `rng_serde` (as `world/mod.rs:384` does), or derive a per-tick `rng_for(seed ^ K, tick)` the way `bots.rs` does. |
| 6 | Med | `power.rs:428`, `history.rs:82,101,147,177`, `crime.rs:182-184`, `services.rs:1266-1267` | Arrays are sized by literal enum counts (`[f32; 7]`, `[u32; 5]`, `[u32; 3]`). Adding a plant kind or offence would make **every existing save unloadable**, because the history ring holds hundreds of these records. Apply the existing `economy::padded` deserializer (`economy.rs:179`) and size the arrays as `Enum::ALL.len()`. |
| 7 | Med | `pedestrians.rs:100-134` | `Walker` (and `Rider`) are saved through `Pedestrians::walkers`, but have no `Default` and no `#[serde(default)]`. `pace`, `room` and `room_leg` were added one commit after walkers became saved (6c792fd → 54cfa15). The next field added to either struct will break saves. Derive `Default` and add `#[serde(default)]` on both. Also fix the doc at line 100 ("never saved"), which is false. |
| 8 | Med | `pillars.rs:171-180` | `way_key` uses `std` `DefaultHasher`, and its value is saved (`Pillar.way`) and matched against fresh keys (`world/pillars.rs:179,291`, `world/plan.rs:1074`). Std does not guarantee the algorithm stays stable across Rust releases. A toolchain bump would make every raised way look new: columns would be replanned and could trigger buyouts. Replace it with an explicit FNV or splitmix over the same quantised `i64`s. |
| 9 | Med | `traffic.rs:2382-2407` | `reroute` calls `insert_sorted(first, id)` even when the car is `entering` (at the kerb) or in a stunt. `pull_out` then inserts it again, so the car appears twice in `lane.cars`. One caller can trigger this today: the fallback in `world/buses.rs:419`. Skip `insert_sorted` when `entering \|\| in_stunt()`. |
| 10 | Med | `zoning.rs:700-711` | `subdivide_sized` at `depth > 60`, or when `cut` fails, pushes the exterior ring of a polygon *with holes*. The parcel then silently covers a lake or an excluded area. Push nothing in those paths, and never emit a polygon whose `interiors()` is non-empty. |
| 11 | Low | `services.rs:855-866, 1266` | `IncidentKind::index()` gives `Crash` = 3, but `incidents_today` and `resolved_today` are `[u32; 3]`, so `get_mut` silently drops crash calls from "calls today" (`world/services.rs:447,2245`). Meanwhile `unanswered_today` *does* count crashes, so unanswered can exceed calls. Count by `slot()` instead. |

### (b) Behaviour-preserving refactors

| # | Where | What |
|---|---|---|
| 12 | `tests/sim_checks.rs:8`, `society_demo.rs:16`, `probe_society_civic.rs:13`, `probe_society_units.rs:16` | Each of these does `#[path = "demo_city.rs"] mod demo;`, which also pulls in demo_city's **8 heavy `#[test]`s**. About 20 sim-days of tests therefore run in 5 binaries instead of 1. Move `demo_world()` and the shared helpers into `tests/common/mod.rs`. This is probably the single biggest test-time saving available. |
| 13 | new `src/hash.rs` | Gather the copies of splitmix / `mix` / `hash01` / FNV into one module, bit-identical to each version they replace (see §2.1). |
| 14 | `geom.rs` | Make geom actually "the one place a formula lives" by moving in: `smoothstep` (×3 in scope), `distance_to_polyline` (×4), `segment_distance` (byte-identical copy at `landscape.rs:2071`), `segments_cross`, `segment_intersection*` (today geom calls back *into* roads), `wrap_angle` / `lerp_angle`, `extents`, `convex_hull` and `bounds` (§2.2). |
| 15 | dead code | Delete the functions with no callers anywhere in the workspace (list in §3.3), and fix the misplaced or stale doc comments (§3.4). |
| 16 | `RoadNetwork` | Add `streets_at(node)`, `away_dir(seg, node)` and `corridor_half_width()`, and `LaneNetwork::opposing(lane)` and `exits_of(lanes)`. Each replaces 3 to 25 hand-rolled copies (§2.3). |

### (c) Bigger restructures

| # | What |
|---|---|
| 17 | **Parcel cutting cache.** `rebuild_from_plan` reruns `zoning::generate_parcels_with` over the whole map on *every* plan rebuild, including zoning-only edits (`world/plan.rs:553, 1146`). As step 1, cache the result on `World`, keyed by `(roads.version, water/landscape id, parcel_half)`. As step 2, memoise per chunk: the chunks are already world-stable by design (`chunk_block`, `block_seed`). |
| 18 | **Road rebuild is O(P·S).** `link` and `split_segment` drop the `OnceLock` grid, so every added piece rebuilds the segment grid. Node lookups scan every node per vertex, and `classify()` runs after every polyline (`roads.rs:486, 601, 833, 878-1036, 939, 958`). Make `geom::Grid` insertable, add a node grid, and run `classify()` once at the end of the rebuild. |
| 19 | **Split the oversized files.** `traffic.rs` (5.3k lines) into about 15 submodules, `zoning.rs` and `landscape.rs` into folders, and `roads.rs`, `walk.rs`, `services.rs`, `power.rs`, `social.rs`, `market.rs` and `committee/proposals.rs` (§3.1-3.2). Do the hash/geom moves (#13, #14) first so the splits are pure moves. |
| 20 | **Determinism and save-compat tests.** Add (i) run `demo_world()` twice and compare `to_ron()` or a hash; (ii) save at N, load, run to N+M, and compare with a straight run (this catches #5); (iii) a checked-in old-save fixture under `tests/fixtures/`, loaded and ticked for a day. Also add a test that every per-kind array covers `Enum::ALL`; it would have caught #1. |

---

## 1. Correctness

### 1.1 Bugs

High and Medium bugs are all in the Top 20 above: #1-#11. These are the remaining smaller ones.

| Sev | File:line | Problem | Fix |
|---|---|---|---|
| Low | `traffic.rs:2938-2947` | At the solid line, a successful reroute does not set `cooldown`. The next substep can reroute back again, running a full A* every substep with no lane change made. | Set `cooldown = SOLID_RETRY` on success too. |
| Low | `traffic.rs:3863, 3922` | `collisions` checks only that the *follower* has not crashed. `crash()` then puts already-crashed cars in a new `CrashReport`, so injuries can be counted twice. | Filter crashed cars first, and return early if none are left. |
| Low | `traffic.rs:2514` (`limit_ahead`) | Road roughness is applied (`limit_at`) only to the current lane; lanes ahead use the raw `speed_limit`. Drivers brake for the smooth limit and then slow again. | Use `limit_at` on the lanes ahead as well. |
| Low | `traffic.rs:2264, 3922` | `spawn_car` indexes `route[0]` and `crash` indexes `cars[0]`, so both panic on empty input. Callers guard against it today. | Add an early return or a `debug_assert!`. |
| Low | `traffic/roundabout.rs` | `lanes_on_ring - 1` on a `u8`. `Ring::new` clamps lanes to at least 1, but a deserialised `Ring` skips the clamp. | Use `saturating_sub`. |
| Low | `landscape.rs:364, 377, 1536` | `River::nearest` and `flow` index `points[0]` and `points[i+1]`, and `WaterGraph::build` computes `ranges.end - 1`. All panic on rivers with fewer than 2 points loaded from a save. | Skip rivers with fewer than 2 points. |
| Low | `landscape.rs:907` | `.max(...)` is applied after `random_range(a..b)`, so it cannot prevent an empty range. Not reachable with today's constants. | Clamp before drawing the range. |
| Low | `landscape.rs:620` | `Flat` presets return early, before `add_hot_spots` and `climate_extras`. Flat + Volcanic gets no hot spots, although its blurb promises them. | Decide whether that is intended; if not, run both before returning. |
| Low | `landscape.rs:1394, 1428` | `sea_way` silently returns a straight `[a, b]` line across land when A* fails. | Return an `Option`. |
| Low | `road_wear.rs:237` | `self.wear[i]` is guarded only by `self.mids.get(i)`, so a save where `wear` is shorter than `mids` panics. A leftover `let _ = k;` sits at line 217. | Use `wear.get(i)`. |
| Low | `level_crossing.rs` `rebuild` | Two new crossings within 3 m of one old crossing both clone its counters. | Mark the old crossing as taken once matched. |
| Low | `world/water.rs:33, 487, 489` | Still uses the pre-`WayClass` `roads::grade_separated`, which does not know that a ramp is separated from a street. | Use `separated(Segment::class())`. |
| Low | `walk.rs:913-960` | Lot gates are matched by edge *midpoint* in 12 m cells (±1 cell). Long collinear edges whose midpoints are more than 24 m apart get no gate. | Index edge bounding boxes in `geom::Grid` and query with `in_box`. |
| Low | `homeless.rs:441-448`, `crime.rs:650` | Money leaves the city with no ledger entry: the meal cost is discarded by `let _ = paid`, and half of the loot vanishes. | Book these sinks, or comment that they are deliberate. |
| Low | `committee/mod.rs:990-996` | The name-dedup loop gives up after 20 tries and pushes a duplicate politician name. | Keep drawing, or add a suffix. |
| Low | `services.rs:~740` (`Placeable::parse`) | Has no names for `MarinaSmall`, `MarinaLarge`, `FishingPortSmall` or `FishingPortLarge`, so the remote API cannot place them. | Add the four names. |

### 1.2 Determinism

**What is clean.**
- **Sorts:** every f32 sort uses `total_cmp` with id tie-breaks. There are no `partial_cmp().unwrap()` calls in scope. `roads.rs:1061` uses `unwrap_or(Equal)` on a stable sort; switch it to `total_cmp` for consistency.
- **RNG:** no `thread_rng` or `SystemTime` anywhere. The only `Instant::now` calls are in tests and benches.
- **HashMap / HashSet:** about 44 uses outside `world/`. All were checked, and all are lookups, counts, or sorted or tie-broken before they affect an outcome:
  - `crime.rs:424`, `trash.rs:789` (sorts `near`), `concerns.rs:445` (`max_by` with a key tie-break)
  - traffic `claims`, `caps` and A* maps
  - `walk.rs`, `zoning.rs` and `roads.rs` lookups

**Latent risks.**
- `traffic/roundabout.rs:83` `RingLanes::entry_node` iterates a `HashMap` to choose a result. It is dead code today (§3.3). Delete it before someone calls it.
- `traffic/roundabout.rs:494, 739`: tests pick a node or lane from HashMap/HashSet order. They are order-independent today but fragile.
- Hot-path stream draws:
  - `Traffic::roll` (splitmix, saved) is drawn per substep in `hazards`, `entered_red` and `crash`. Any new caller shifts every later crash and reshuffles `drunk_drivers_swerve_more_and_crash_more_often` (`traffic.rs:4972`).
  - `recruit_offenders` and `hourly_trash` draw from the society stream once per eligible adult per hour.

  Per CLAUDE.md, prefer `hash01(entity, tick, salt)`, as `siren::pulls_over` already does.
- `pillars.rs:171`: `DefaultHasher` in a saved key (Top 20 #8).

### 1.3 Serde

- **Fields added later:** every field added after its struct was created has `#[serde(default)]` or a container default. Checked with `git log -S` by three reviewers across traffic, zoning, landscape, ports, shore, towers, weather, pillars, economy, market, services and social. One gap: `Walker` and `Rider` (#7).
- **Enum order:** RON stores variants by name, so order does not matter for saves. Where `index()` addresses a saved array, every variant was appended at the end (checked).
- **Real hazards:**
  - Literal-length arrays (#6).
  - `social.rs:188` `TieRepr` and `social.rs:548` `NoteRepr` are tuple-serialized, so a new field breaks old saves. Add a comment at the type stating the rule, or deserialize a variable-length sequence the way `economy::ledger_row` does.
  - `Ground::fix` (`landscape.rs:1947`) is kept in non-test builds under `#[cfg_attr(not(test), allow(dead_code))]`: an n² `Vec<Fix>` of about 810k entries kept alive in release builds. Keep it behind `cfg(test)`, or drop it after the build.

---

## 2. Duplication

### 2.1 Hashes → new `src/hash.rs`

The splitmix constant `0xBF58_476D_1CE4_E5B9` appears in 16 files. The identical or near-identical copies are:

| Copy | Variant |
|---|---|
| `committee/mod.rs:968` `mix`, `corp.rs:319` `mix`, `world/nights.rs:14`, `world/social.rs:108` | splitmix64 finaliser of `a ^ b·φ`, identical |
| `streets.rs:91` `mix` | same maths, written out longhand |
| `shore.rs:137` `mix`, `world/waterfront.rs:52` `hash` | `a·φ ^ b·C2B2…`, two-round variant, identical |
| `fishing.rs:249` `hash` | same as `shore::mix`, plus `^ 0xF15F_15F1` |
| `pedestrians.rs:92` `hash01` | three-input version returning a unit float |
| `corp.rs` `unit` | hash → `0..1` |
| `zoning.rs:954` `polygon_seed`, `zoning.rs:1712` `block_seed`, `committee/mod.rs:1042` | inline FNV loops |

Proposal: `hash::{mix, mix2 /* shore variant */, hash01, unit, fnv}`. Keep each variant's output bit-identical so that generated content does not change, then replace the copies one by one.

### 2.2 Geometry → `geom.rs`

| Helper | Copies | Action |
|---|---|---|
| `smoothstep` | `landscape.rs:513`, `ground.rs:100`, `industry.rs:380`, `shore.rs:84` (closure), `fishing.rs:635`, inline `landscape.rs:941`; also nd_procgen `terrain.rs:41`, `textures.rs:27`, `street.rs:381` | `geom::smoothstep` (or a `noise.rs` with `hash2`, `value_noise` and `fbm`) |
| Point to polyline distance | `streets.rs:255`, `landscape.rs:602`, `fishing.rs:567` `to_line`, `tests/lane_joins.rs:10`; the projection loop in `transit.rs:199-225` | `geom::distance_to_polyline`, built on `project_on_poly` |
| Segment to segment distance | `landscape.rs:2071` is byte-identical to `geom.rs:560` | Delete the copy. Also move `geom.rs:560` above `mod tests`. |
| Segment intersection / crossing | `roads.rs:1526, 1541`; `zoning.rs:261` `segments_cross`; the closure inside `geom::self_intersects`; `walk.rs:1646` (test) | `geom::segments_cross` / `segment_intersection`, re-exported from roads. This also fixes geom importing from roads. |
| Bounds | `streets.rs:259` (identical to `geom::bounds`), inline folds at `zoning.rs:1098`, `traffic/remap.rs:72`, `shore.rs:164` (re-inlines `zoning::extents`) | `geom::bounds`, `geom::extents` |
| Polyline length | `roads.rs:1422` `path_len`, `fishing.rs:666` `path_len` (an alias), `streets.rs:47, 316` | `geom::path_length` |
| Polyline projection and points along it | `Lane::project` (`traffic.rs:346`) is the same as `geom::project_on_poly`; `Lane::point_at` (`traffic.rs:317`) is the same as `geom::point_along`, but with cached cumulative lengths | Add `geom::point_along_cum(pts, cum, s)` |
| Angle wrap and lerp | `traffic.rs:1053` `lerp_angle`, `traffic/scene.rs:169`; also nd_app `render.rs:2140` and `camera.rs:298` | `geom::wrap_angle`, `geom::lerp_angle` |
| Convex hull | `estate.rs:596`, nd_procgen `buildings/construction.rs:969` | `geom::convex_hull` |
| Bilinear raster sampling | `landscape.rs:1144` and `:2611` (identical maths); `ground.rs` `Contamination::at` (cell-centred variant); cell-range closures at `landscape.rs:1042, 2262`; `land_value.rs:205, 213, 225` repeat `index()` (line 167) | `geom::HeightGrid { origin, cell, n, data }` with `bilinear()` and `cell_range()` |
| Water grid path + string-pull | `landscape.rs:1362` `sea_way` (30 m cells, A*), `fishing.rs:435-565` `WaterGrid` (20 m cells, Dijkstra) | One `landscape::water_grid`, cached on `Landscape` |

### 2.3 Roads, lanes and levels → `RoadNetwork` / `LaneNetwork`

| Helper | Copies | Home |
|---|---|---|
| "Alive street segments at a node" filter | `traffic.rs:1386, 1485, 1717`; `traffic/junction.rs:42, 81`; about 20 more in `walk.rs` and `world/*` | `RoadNetwork::streets_at(node)` |
| Number of street arms at a node | `walk.rs:321` `street_arms` | Identical to `RoadNetwork::street_degree` (`roads.rs:870`); delete it |
| Unit direction of a segment away from a node | `traffic.rs:1394, 1573-1579`, `bend_cap` ~1725; `junction.rs:36`; `remap.rs:243`; `level_crossing.rs:470`; `walk.rs:374, 558, 589`; `streets.rs:163` | `RoadNetwork::away_dir(seg, node)`. The copies disagree: some pick the nearest endpoint, others test `seg.a == node`. Choose one rule. |
| Corridor half-width | `zoning.rs:381` (closure), `zoning.rs:446`, `landscape.rs:1860`; `landscape.rs:2214` (a `+0.5` variant) | `Segment::corridor_half_width()` |
| Lane half-width formula | `plan.rs:321` (builds a fake `Segment`), `roads.rs:1283` `Ring::half_width` | `roads::half_width_spec(kind, fwd, bwd)` |
| Opposing lane | `traffic/hazard.rs:123`, `traffic/siren.rs:171` (identical) | `LaneNetwork::opposing` |
| Exit mask of an approach | `traffic.rs:1559`, `junction.rs:98`, `roundabout.rs:203` | `LaneNetwork::exits_of` |
| Default signalled-by-shape rule | `traffic.rs:1604`, `junction.rs:87` | One `default_signalized()` |
| Next car on the route | `traffic.rs:3163` (`blocker_ahead`), `traffic/why.rs:247` | One `Traffic` helper |
| Drive towards a point | `traffic.rs:4101` (closure in `step_stunt`), `traffic/scene.rs:~158` | One `drive_toward()` |
| Old-to-new segment matcher | `road_wear.rs` and `road_traffic.rs` (same constants, `cell()`, 3×3 search) | One shared matcher, which is also where #4 gets fixed |
| Nearest lot | `world/services.rs:395` `coverage_at` (linear scan), `concerns.rs:494` | Backed by `geom::Grid` |
| Nearest road | `zoning.rs:452` `nearest_road` wraps `nearest_where` | Fine; keep it, as it only adds a filter |

### 2.4 Other

- **Personality traits from a seed:** `places.rs:253`, `vehicle.rs:697` `traits_of`, `vehicle.rs:201` `hedonism_of` (equal to `traits_of[2]`), `looks.rs:273`, `citizens.rs:467, 477`. `social::compatibility` reseeds ChaCha four times per conversation. Keep one version, and cache the traits on `Citizen`.
- **Household class by income:** hand-rolled at `trash.rs:318, 351`, `crime.rs:795` and `world/trips.rs:405, 632, 653`. Make `world/pressure.rs:97` `household_class` `pub(crate)` and use it everywhere.
- **Tier parsing:** `services.rs:800`, `committee/proposals.rs:86, 95`. Move to `Tier::from_key` and `Tier::key` in `buildings.rs`.
- **Constants and wrappers:**
  - `committee/report.rs:234` repeats `services::COVERED`.
  - `economy.rs:154` `ledger_row` is just `padded::<f32, FLOWS>`.
  - `committee/rules.rs:224` `capitalise_first` only wraps `capitalise`.
  - A raw `1440` appears instead of `MINUTES_PER_DAY` at `crime.rs:377`, `homeless.rs:499, 520`, `services.rs:182` and `enforcement.rs:63`.

---

## 3. Structure

### 3.1 traffic.rs (5349 lines: ~4140 code, ~1210 tests)

Keep `traffic.rs` as the module root: constants, `Traffic`, ids, re-exports and a slimmed `step`. Move the rest out by topic.

| New module | Contents (current lines) |
|---|---|
| `traffic/lane.rs` | `LaneKind`, `Lane`, `LaneToll`, `TollPass`, `impl Lane` (202-375); `cubic_join`, `connector_curve` (1285-1368) |
| `traffic/build.rs` | `LaneNetwork`, `Intersection`, `Approach`, `rebuild`, `push_connector`, `set_arrows` (377-472, 1370-1620, 1809-1847) |
| `traffic/ramps.rs` | `ramp_profiles` (1622-1807) |
| `traffic/arrows.rs` | `turn_bit`, `arrows_label`, `parse_arrows`, `default_arrows` (473-556); `set_lane_use`, `adapt_arrows` (2751-2805) |
| `traffic/signals.rs` | `apply_signals`, `step_signals`, `SignalDemand`, `signal_demand`, `oncoming` (1849-1928, 2646-2716) |
| `traffic/routing.rs` | `nearest_lane_on` … `route_length`, `Open` (1930-2204); `replan_from_here`, `redirect`, `route_from_here`, `follow`, `reroute` (2382-2480); `reroute_stalled` (3060-3105) |
| `traffic/vehicle.rs` | `Cargo`, `Car`, `Wait`, `Bus`, `impl Car` (557-700, 1063-1196) |
| `traffic/driver.rs` | `DriverProfile` … `CrashReport`, `road_rage`, crash constants (702-1050) |
| `traffic/kerb.rs` | `insert_sorted`, `spawn_car`, `pull_in`, `pull_out`, `kerb_gap` (2257-2380) |
| `traffic/follow.rs` | `idm_with`, `exit_blocked`, `at_junction`, `limit_ahead`, `lookahead`, `rail_gap`, `next_turn`, `waiting_left`, `pass_offset` (2206-2213, 2482-2741) |
| `traffic/lane_change.rs` | `neighbours_at`, `change_ok`, `change_lane`, `lane_changes`, `swap_lanes`, `drop_dead_end_changes` (2807-3022, 3234-3250) |
| `traffic/gridlock.rs` | `Gridlock`, `gridlocks`, `blocker_ahead`, `wait_cycles` (1172-1184, 3107-3232) |
| `traffic/tolls.rs` | `toll_cap`, `toll_gates`, `toll_penalty`, `clear_tolls` |
| `traffic/crash.rs` | `pull_aside_from_pile`, `back_a_lane`, `roll`, `runs_red`, `entered_red`, `collisions`, `hazards`, `crash` (3709-3950) |
| `traffic/stunt.rs` | `set_blocked`, `begin_stunt`, `end_stunt`, `step_stunt` (3951-4007, 4100-4137); could merge with `scene.rs` |
| `traffic/place.rs` | `place_cars` (4009-4082) |
| `traffic/tests/*.rs` | The tests, split by topic |

`step` (3252-3645, about 400 lines) should become `plan_substep() -> Vec<Step>`, `apply_steps()`, `advance_lanes()` and `reorder_passers()`. That also makes bug #2 structurally impossible.

### 3.2 Other oversized files

| File | Split into |
|---|---|
| **zoning.rs** (2374) → `zoning/` | `mod.rs` (types, 1-250); `poly.rs` (geo bridges, cut, canonical, simplify; the pure-Vec2 parts go to geom); `frontage.rs` (road_chains … reach_pieces); `subdivide.rs` (621-953, 1712-1757); `water_mask.rs` (982-1075); `lots.rs` (972-1282); `parcels.rs` (1300-1782); `join.rs` (1853-2033) |
| **landscape.rs** (3076) → `landscape/` | `params.rs` (150-327); `features.rs` (329-475); `noise.rs` (`hash2`, `value_noise`, `fbm`, smoothstep; shared with ground, weather and fishing); `generate.rs` (556-930); `raster.rs` (937-1320); `routing.rs` (1320-1790); `built_ground.rs` (1790-2677). Renaming `landscape::Ground` there removes its clash with the `crate::ground` module. |
| **roads.rs** (1890) → `roads/` | `mod.rs` (types and queries), `grade.rs` (`WayClass`, `separated`, crossings), `ramps.rs` (`classify`, ease ramps, `sample_heights`), `build.rs` (`add_polyline*`, node lookups), `ring.rs`, `path.rs` (A*), `tests.rs` |
| **walk.rs** (1779) → `walk/` | `setbacks.rs` (also used by traffic and procgen), `build.rs`, `footbridge.rs`, `grounds.rs`, `route.rs` |
| **rail.rs** (1109) | `facility.rs`, `train.rs`, `freight.rs`, `track.rs` |
| **services.rs** (1624) | Move `Placeable` and `BuildCategory` to a top-level `placeable.rs` (they span power, rail, ports and tolls); then `services/{kinds, incident, units, stats, chase}.rs` |
| **power.rs** (1232) | `plants.rs`, `battery.rs`, `dispatch.rs`, `load.rs` |
| **social.rs** (1186) | `ties.rs`, `talk.rs`, `groups.rs`, `mod.rs` |
| **market.rs** (1108) | `resource.rs`, `business_kind.rs` (the ~470-line tables), `business.rs`; move `Hours` to `clock.rs` |
| **committee/** | Pull `pool.rs` and `types.rs` out of `mod.rs` (1084). Split `proposals.rs` (1106) into `actions.rs`, `suggest.rs`, `sites.rs`, `implement.rs`. |
| **bots.rs** | Its `impl World` systems (`dispatch_deliveries`, `step_bots`, `settle_delivery`) belong in `world/bots.rs` |

Several type names are reused across modules: `Pressure` (crime, pressure, estate), `Outcome` (services, social, committee), plus `News`, `Spot`, `Tally`, `Stage` and `Record`. Disambiguate them when splitting.

### 3.3 Dead code

Each item was grepped across all of `crates/`, including nd_app and the examples, and has no callers.

| Area | Items |
|---|---|
| traffic | `Traffic::clear_cars` (`traffic.rs:2226`; superseded by `rebuild_carrying`, and its doc is stale); `Traffic::clear_crash` (3943; its doc contradicts the body); `RingLanes::entry_node` (`roundabout.rs:83`); `let _ = road_lane_count` (1611); the redundant `node_intersection.resize` (1605); the always-true `i < self.cum.len()` (338) |
| roads, walk, air | `roads::passes_over` (67; superseded by `passes_over_class`); `WalkGraph::bench_in` (`walk.rs:1178`); `Air::is_pad` (`air.rs:422`) |
| zoning, landscape, fishing | `zoning::generate_parcels` (1473; only referenced in doc links); `Ground::bridge_at` (`landscape.rs:2586`) and `Ground::bridge_decks` (1959); `zoning::generate_lots_wet` (its only caller passes an empty `WaterMask`, so merge it into `generate_lots`); `fishing::path_len` (an alias of `geom::path_length`) |
| other modules | `committee/mod.rs:412` `title_line`; `services.rs:1501` `chase_of_car`; `estate.rs:551` `convoy_of_car`; `estate.rs:556` `nimby_at`; `homeless.rs:150` `camp_people`; `concerns.rs:486` `complain_about`; `history.rs:132` `supply_total_mwh`; the `None` arms in power `size_range` and `size_scale` (`reference_area` always returns `Some`) |

### 3.4 Stale or misplaced comments

| Area | Location and problem |
|---|---|
| traffic | `traffic.rs:190-192`: an orphaned "trips older than this end where the car stands" sits on `SIREN_SPEED_FACTOR` and contradicts `REPLAN_AFTER`. |
| traffic | Docs glued to the wrong function: `traffic.rs:332` (`project`'s doc is above `points_between`), `1158` (`change_pending`'s is above `glide_at`), `2509` (`lookahead`'s is above `limit_ahead`), `traffic/crosswalk.rs:231`. |
| road_wear, road_traffic | `road_wear.rs:203` and `road_traffic.rs:130` promise that a split keeps wear on both halves, which is false (#4). |
| roads | `roads.rs:1273` says "1 or 2" ring lanes, but `RING_MAX_LANES = 3`. `roads.rs:359` has a path doc on `RAIL_HALF_WIDTH`. |
| pedestrians | `pedestrians.rs:100` says walkers are "never saved", which is false. `pedestrians.rs:131` has the `CrossingInfo` doc on `Rider`. |
| walk, rail | `walk.rs:1059` (footbridge doc misplaced). `rail.rs:813` (misplaced doc). `rail.rs:385` says "within a degree", but no angle is checked. `rail.rs:290` has a rambling half-sentence. |
| subway | `subway.rs:328` says "ties go to the shorter walk", which is not implemented, and the fourth tuple field is always 0. |
| zoning, landscape, geom | `zoning.rs:74` has `lot_width`'s doc on `fronts_paths`. `landscape.rs:2584` describes a caller that does not exist. `geom.rs` claims to be "the one place a formula lives" (§2.2 shows otherwise). |
| market, power | `market.rs:852-854` says of `wants_expand` "nothing acts on it yet", but `world/estate.rs:621` acts on it. `power.rs:222` says "`None`: a plant of fixed size", but no kind returns `None`. |

---

## 4. Performance hot spots

### Traffic and pathfinding

- **Routes:** a route is planned once per trip and kept in `Car::route`. That is good: A* is not rerun every tick.
- **Replans** happen on a stall (backed off up to 320 s), at the solid line (see the Low bug in §1.1), once per overtake candidate per substep after the gain test, and at `REPLAN_AFTER`.
- **A* costs:**
  - Stale-entry re-expansion (#3).
  - Two `HashMap`s allocated per call (`traffic.rs:2029-2030`). Use generation-stamped `Vec` scratch buffers on `LaneNetwork`.
  - Per expansion, `rings.circulating.contains` plus `junction_after()` walks up to 16 lanes. That "room before the solid line" depends only on the network, so precompute it per lane in `rebuild`.
- **No reachability precheck:** `world/cars.rs:363, 440, 489` and `world/driving.rs:700, 879-883` try several goals with `find_map(route_lanes)`. Each unreachable goal costs a full search of up to 20,000 expansions. Store strongly connected component ids per lane in `rebuild` and reject unreachable goals in O(1).
- **Per-substep allocations:**
  - `signal_demand` (`traffic.rs:2670`): 5 `Vec`s per intersection.
  - The `claims` `HashMap` (3276) and `accs`, plus three `cars.keys().collect()`.
  - `neighbours_at` (2809) and `kerb_gap` (2360) collect a `Vec` per call.
  - `ring_blocks_entry` (`roundabout.rs:364`): a `HashSet` per entering car.

  Use scratch buffers on `Traffic`.
- **Quadratic work per lane:** `lane.cars.iter().position(..)` per car in `lane_changes` (2963), `hazards`, `hazard_ahead` and `plan_hazard_passes` is O(k²) per lane per substep, which hurts on long queued highway lanes. Iterate with indices. `arrived.contains` (3539) is a small extra O(n²).
- **Rebuild:** `remap::lanes_near` (`traffic/remap.rs:65`) scans every lane for each reshaped car. Build a `geom::Grid` once per `rebuild_carrying`.
- **UI:** `why::stop_reason` calls `scene_zones()` (clone plus a scan of all cars) per `Wait::Blocked`, and `jam_chain` calls it up to 400 times. Compute the zones once per chain.

### Roads, walk and trips

- **Road rebuild is O(P·S)** (#18).
- **`world/trips.rs:1381`:** `walk_or_ride` runs `roads.foot_path()` (an A* with two HashMaps and no stale skip) on *every* walking trip, even when `walk_route_for` succeeds and the result is never used. Compute it lazily.
- **Walk graph:** fully rebuilt on every lot add or remove (`world/mod.rs:821`, from `placement.rs:174, 298, 323` and `construction.rs:259`, which runs mid-sim). Acceptable for now; make it incremental per lot when it shows in profiles. `park::plan_parks_with` (`park.rs:156, 167`) is O(L·P) plus an O(P²) shared-edge pass with no bounding-box test on every `rebuild_walk`. Add a `BTreeSet` and a `boxes_overlap` pre-check.

### Parcel cutting

- **Full recut on every plan rebuild** (#17).
- **`zoning.rs:850-863`:** `subdivide_sized` computes `from_geo` + `frontage_runs` three times per candidate cut. Each call does a `nearest_where` per edge, inside a loop that runs for every piece, recursively. Compute it once per piece.
- **`world/plan.rs:474, 1156`:** `shrink_about_centroid(fp)` is reallocated for every parcel × footprint pair, with no bounding-box pre-test.
- **`zoning.rs:1914` `join_parcels`:** recomputes bounds inside its O(n²) loops and clones the group polygon on every iteration.

### Landscape and water

- **`landscape.rs:1322-1460`:** `water_route`, `water_routes_out` and `sea_way` rebuild a `WaterGraph` and a 124×124 exact `is_water` grid on *every call*. `water_at` has no bounding-box reject for rivers (line 954). The taut-string pass is O(path²) in line checks. Callers run per ferry sailing (`world/water.rs:1372, 1403, 1409`) and O(terminals²) in `shipping_lanes` (`world/fishing.rs:58`). Cache both in `OnceLock`s on `Landscape`, as `raster` already is.
- **`build_raster`** (1065-1110) allocates an n² buffer and runs an n² point-in-polygon pass per lake and island. It runs lazily on the first `height()` call, which can cause a hitch mid-tick. Scan only each feature's bounding box.
- **`Ground::outer_height`** (2509) allocates per call, and is called per cell and by the renderer. **`deck_at` / `decks_at`** (2563-2608) are linear scans.
- **`lot_ground_problem`** (1219) samples every height twice.

### Society (hourly and daily passes)

- **`World::coverage_at`** (`world/services.rs:395`) is a linear scan over all lots. It is called per offender per hour (`crime.rs:472`), per homeless adult per hour (`crime.rs:302`) and twice per homeless household per day (`homeless.rs:510`). Back it with `geom::Grid`.
- **Other linear scans:**
  - `services.in_custody`: linear, per adult (`crime.rs:436`). Use a `BTreeSet`.
  - Per camp per hour: `lots.values().any(contains)` (`homeless.rs:420`).
  - Per offence: a scan of every building (`crime.rs:488`) and of every citizen (`crime.rs:543`).
  - Per homicide arrest: `format!` inside a closure over every memory (`crime.rs:746`).

---

## 5. Tests

### Structural problems

1. **demo_city tests run 5 times** (#12). `tests/sim_checks.rs`, `society_demo.rs`, `probe_society_civic.rs` and `probe_society_units.rs` all `#[path]`-include `demo_city.rs`, together with its 8 `#[test]` functions. That multiplies runtime by 5, and multiplies the borderline `late < 0.085` check by 5 as well.
2. **The demo layout exists in about 10 copies, and the example copies are stale.**
   - Copies: `tests/demo_city.rs:13`, `nd_app/src/sim.rs:130`, `landscape.rs:2886` (`demo_ground_world`), and the examples carlife, freight, parking, land, jam, peak, traffic, perf and headless.
   - **Every example copy places the coal plant with `max_by(area)`.** The app and the tests now use the lot nearest `power::COAL_REFERENCE_AREA`, so the probes measure a different city. That includes `headless`, which the README advertises.
   - Fix: `pub fn nd_sim::scenarios::demo_district(&mut World)`, used by the app, the tests and the examples.
3. **No whole-world determinism test and no save → load → continue test.** Determinism is only checked per subsystem (`car_tests.rs:332`, towers, air, chases, level crossings, roundabouts). `tests/sim_checks.rs:81-84` admits that vehicles do not yet move identically after a load. This gap hides #5.
4. **No old-save fixture.**
   - Compatibility tests strip fields out of freshly generated RON.
   - `committee/tests.rs:164` uses `rfind("committee:")` and truncates everything after it, which also removes `shore`, `path_links` and `corps`. The test therefore does not prove what it claims. Edit a parsed RON value instead.
   - `tests/rebuild_regressions.rs:124-149` is `#[ignore]` and silently *passes* when `ND_SAVE` is unset. Make it `panic!`.

### Fragile tests

| Test | Problem |
|---|---|
| `tests/society_demo.rs:226-239` | 911 thresholds tuned to one seed (`median <= 15` was already loosened); strict `served < bare` comparisons between two separate runs |
| `tests/demo_city.rs:145, 148, 160, 315` | Thresholds such as `late < 0.085` against a measured 8.07%, on a documented seed spread of 1-12% |
| `traffic.rs:4972` | Drunk-driver test: statistics over 4 seeds of the shared `roll()` stream |
| `landscape.rs:2729` `desert_output_is_pinned` | Hashes the `Debug` text of floats produced by `sin`/`cos`/`exp`, so libm or platform differences can break it. It actually pins every preset. |
| `traffic/roundabout.rs:494, 739` | Pick from HashMap/HashSet order |

Fixes:
- Assert over windows of days or over K seeds.
- Print the margins, so drift is visible before a check flips.
- Pin quantised values instead of `Debug` text.

### Not real tests

| Test | Problem | Fix |
|---|---|---|
| `zoning.rs:2333` `dump_s_curve_parcels_for_inspection` | No assertions; runs a full generation on every test run | `#[ignore]` it, or move it to `examples/` |
| `walk.rs:1721` `dump_walk_graph_for_inspection` | No assertions | `#[ignore]` it, or move it to `examples/` |
| `tests/demo_city.rs:217-262` | The doc promises tickets and tows, but the test only prints | Assert `tickets_total > 0` and so on |
| `tests/society_demo.rs` | Computes homeless, camps, arrests and chases, but asserts none of them | Add assertions |
| `tests/probe_society_units.rs:49-58` | Hand-rolls a bulldoze that bypasses `World::bulldoze_building`, so it tests a state the player can never reach | Use `bulldoze_building`, and drop the `catch_unwind` |
| `tests/sim_checks.rs:52` | Duplicates `demo_city.rs:394` (the stricter version) | Delete it |

### Gaps

| Area | Missing coverage |
|---|---|
| traffic | Replan with `idx > 0` (#2); direct `route_search` tests (avoid lane or step, the expansion cap on a big grid, cost optimality); `reroute` of an `entering` car (#9); `back_a_lane` / `pull_aside_from_pile`; `kerb_gap` sliding; `parse_arrows` ↔ `arrows_label` round-trip; unit tests for `remap.rs` |
| roads and level of service | road_wear and road_traffic `remap` on split, merge and renumber (#4); `junctions.rs` (none at all); `plan.rs` `carry_levels`, `sample_curve_levels`, insert or remove point with levels; `streets.rs` twin naming and loops |
| zoning and landscape | Chunk invariance of `generate_parcels_with` ("road works elsewhere never move a parcel here"); holes (#10); `join_parcels` / `grow_site` limits; `WaterMask`; robustness to degenerate rivers; `Ground::height` off the raster; stability of `way_key` (#8) |
| society modules | No module tests for homeless (565 lines), trash (896), crime (837), bots, ports or `committee/{report, rules}`. Today they are covered only through the borderline 8-day demo. Build small-town fixtures for the camp lifecycle, trash routing and offender → arrest → reoffend. |
| enum coverage | Every per-kind array has length `Enum::ALL.len()` (#1, #6, #11) |

### Examples (probes)

All 24 compile. None asserts anything; that is fine for the ones that inspect saves.

- **Stale:** every demo-based probe uses the old coal plant placement (item 2 above). `land_probe.rs:1` claims to match `tests/demo_city.rs`, which is no longer true.
- **Superseded or duplicated:**

  | Probe(s) | Action |
  |---|---|
  | `parking_probe` | Covered by `demo_city.rs:221`; delete it |
  | `ped_roads_probe` | Its walk-over-motorway scan duplicates `world/ped_highway_tests.rs:103`; fold the rest into `junction_dump` |
  | `land_probe`, `land_run`, `land_site` | Merge into one `land_probe {demo\|run\|site}` |
  | `jam_probe`, `peak_probe` | Merge, sharing the demo setup and the stop-reason reporting |
  | `carlife_probe` | Its invariants are already in `demo_city.rs:173`; keep it only for the mode-share print |

- **Should become tests:**
  - `freight_probe`: todo.md:335 notes "longest wait 2 h". Assert under 3 h over 3 demo days.
  - The teleport check from `jump_probe` / `draw_probe`: an invariant over all cars for a demo day.
- **Robustness:** `crossing_probe`, `ped_roads_probe`, `junction_dump`, `ring_probe`, `island_probe`, `ramp_probe` and `save_probe` index `args[1]` and `args[2]` with no usage message.
- **Docs:** list the surviving probes with a one-line purpose each (CLAUDE.md says only "probes in `examples/`"). Add `cargo check -p nd_sim --examples --features profile` so `perf_probe` stops being skipped.
- **`tests/source_encoding.rs`** guards against mojibake. Keep it.
  - It only scans `.rs` files; extend it to `.ron` and `.md`.
  - Move its self-skip check before the file read.
  - Consider moving it to a workspace-level test.
