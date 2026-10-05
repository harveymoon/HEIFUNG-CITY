# Code review — District 2040 at commit 97e3326 (October 2026)

Four independent read-only reviews were run in parallel over the whole workspace
(nd_sim core; nd_sim networks and geometry; nd_procgen; nd_app and project-level),
each with its own throwaway probe harness where a claim needed measuring. This file
consolidates them. Items marked **FIXED** were corrected in the commit that adds this
file; everything else is open and ranked so the next passes can be planned from here.

Baseline: `cargo test --release`: 51 tests, all passing (now 57 after the fixes below).
Build warnings: 5 (unused struct fields in render tags, `Unit.trailer`).

---

## 1. What is solid

- **Engine-free, deterministic simulation.** `nd_sim` has no Bevy in it, one seeded
  ChaCha8 RNG whose stream and position are serialised, salted sub-RNGs for parcels and
  buildings, no HashMap iteration feeding randomness. Procgen output is byte-identical
  across runs (verified).
- **Clean sim/app boundary.** `SimEvent` for frontend reactions, `MindProvider` with
  non-blocking decide/poll, derived state (`lanes`, `walk`, `trains`, `effective_plan`)
  marked `#[serde(skip)]` and rebuilt in `after_load`.
- **Fixed sim clock with interpolation** (prev/next poses, `Sim::frac`) gives smooth
  motion independent of frame rate; the tick budget with backlog drop is the right
  default for a city builder.
- **Geometry is defensive.** `cut()` self-checks area conservation, corridors are real
  polygon differences, the containment toolkit (`offset_poly` with mitre limit,
  `fit_inside`, `clamp_into`, `interior_anchor`) survived ~2,200 builds on triangles and
  slivers without a panic; heaps use `total_cmp`, A* has an expansion cap.
- **Tests read as specifications** (right-hand lanes, parcels on all four sides of a
  block, expropriation tolerance, rail fusing, construction stalls) and the style sweep
  (every style × land use × ~100 odd lots × 3 tiers) is a strong harness.
- **Documentation** (README, DESIGN.md, module docs explaining *why*) is unusually
  complete for a project this size.

---

## 2. Fixed in this pass

| Area | Finding | Fix |
|---|---|---|
| zoning | Any single-sided frontage ≥ ~400 m parcelled into **zero** usable lots: the cut-walk demanded compactness of the remainder strip, which a long strip can never meet. | Compactness is asked only of final pieces; runs over 8 lots are halved first so recursion stays shallow. Test `long_frontages_parcel_all_the_way_along` (400 m and 1200 m, all six land uses). |
| rail | `implement()` **panicked** when the track a station fused to was bulldozed: `refresh_rail_links` indexed the old `SegmentId` into the renumbered network. | Fresh `RailLink::beside` is found first; the stale id is only consulted through the new `RoadNetwork::try_segment`. Test `bulldozing_the_track_under_a_station_disconnects_it_without_panicking`. |
| roads | A road drawn **through an existing node** (junction or polyline vertex) never joined it, because the crossing test excludes its own ends. | `add_segment_between` also projects every joinable node onto the new way and splits there. Test `a_road_drawn_through_a_junction_joins_it`. |
| roads | Interior street vertices snapped onto rail-only nodes (bogus level crossings). | Kind-aware `get_or_add_node_kind`; `end_node` fallback likewise. Test `a_street_vertex_near_the_track_does_not_make_a_crossing`. |
| market | A truck removed by a network rebuild or load **lost its cargo**; the buyer's `inbound` stayed inflated, so it never reordered and starved; a construction site's `ordered` flag stuck. | `strand_car` delivers the cargo before stranding. |
| economy | Daily tier up/down applied to plants, depots, stations, yards and parking lots. | `!special` guard on the tier branch. |
| economy | Closed businesses were taxed on stale `revenue_today` every day until demolition. | Tax line moved after the closed check. |
| citizens | `arrive` walked into demolished buildings (ghost "at home"); the "no way through" bail-out left `inside` set while visible. | Dead destination → Idle with a thought; bail-out calls `leave_building`. |
| citizens | `emigrate` leaked `visitors` and parking spaces. | Releases both before removing the citizen. |
| transit | `remove_bus_stop` shifted bus indices but not riders' `board`/`alight`. | Riders' indices shift; trips through the removed stop are cancelled. |
| procgen | **NaN** normals/UVs from degenerate quads (zero-size trees on GardenTower, nuclear dome apex ring): ~76k NaN vertices per 1,900 builds, breaking tangent generation for whole surfaces. | `quad`/`tri_facing` skip zero-area faces, `uv_basis` has a fallback, dome apex keeps a small radius, the two zero-size `tree()` calls removed. Sweep test now asserts finite normals and UVs. |
| procgen | `frustum` end caps faced inward for horizontal/descending solids (pipes, beams, ramps, jetties). | Caps face away from the solid's centroid. |
| procgen | Nuclear containment overran lots narrower than its minimum radius. | Radius clamped to the distance to the yard boundary. |
| app | egui kept keyboard focus on the last clicked widget and **re-clicked it on Space/Enter**: Space to pause also toggled the last checkbox; Enter to finish a road after clicking Load reloaded the city. | Focus is surrendered after the panel each frame (no text fields exist yet; gate on `wants_keyboard_input` when they do). |
| app | **Right-drag to orbit committed** the road/zone being drawn (finish fired on right *press*). | Finish fires on right *release* with < 6 px of travel and not over the UI. |
| app | `E` was both the Edit hotkey and camera yaw; holding it to rotate discarded the road in progress. | Letter alias removed (digits remain). |
| app | `sync_overlay`/`sync_paths`/`sync_transit` ignored `full_rebuild`; citizen/car/train entity maps survived Load, so reused slotmap keys kept the old mesh (car for bus). | All three honour the flag; mover maps are drained on a full rebuild. |
| app | Rail-tool gizmo was O(buildings² × facilities) with a `ptr::eq` hack. | Direct lookup by building id. |

Also verified **not** a bug: hotkeys work with the pointer over the side panel. The
earlier failure was the screenshot script sending keys to an unfocused window.

---

## 3. Open findings, ranked

### Critical / High

1. **Overlapping zones produce overlapping lots** (`zoning.rs` ~1028). A newer zone wins
   only where the *centre* of an older parcel falls inside it; two parcels can share
   most of their area. Subtract the union of covering zones from the zone polygon
   before `subdivide` (geo is already a dependency).
2. **Trains are untouched by a rebuild** (`world.rs` `rebuild_from_master`). After track
   is removed or re-smoothed a train keeps its old polyline (ghost over fields), stale
   `branch` arc and dead node ids. Remove trains on rebuild (stranding riders like cars)
   or re-plan from the nearest surviving stop; use `buildings.get` for `stops`.
3. **Rail imports queued at a yard that loses its connection are stranded forever**
   (`import` → `rail_inbound`; dispatch only to yards whose link `works()`). Drain by
   truck when the yard is disconnected or no freight has run for two headways. Also:
   with a yard present, construction parts wait 3–6 h per quarter, roughly quadrupling
   build time versus the truck path.
4. **Time-scale mismatch.** Motion runs at 1.5 traffic-seconds per sim minute while
   every schedule is in clock minutes: a 500 m walk takes ~4 h of clock, people leave
   for work and arrive at closing time, hunger is driven by walking time. `walk_speed`
   and `bike_speed` also hard-code the 1.5 instead of reading `traffic_seconds_per_tick`.
   Decide the compromise explicitly (one `metres_per_tick` helper, scaled schedule
   constants) and add a test: a 300 m commute at 08:00 arrives before 09:00.
5. **Renderer scaling: one mesh + entity per (building, surface), tangents by mikktspace
   per mesh.** Measured 7.2 meshes and ~234 KB of vertex data per building; a 2,000-
   building city is ~14k meshes, ~4.3M tris, ~460 MB CPU-side (plus the GPU copy,
   `RenderAssetUsages::default()`). Emit tangents from `uv_basis` in the generator,
   batch per surface per ~128 m chunk with `Geometry::append` (vacant lots already do
   this), use `RENDER_WORLD` only, compress vertex colours.
6. **`sync_lots` regenerates every vacant lot and every park whenever land value
   recomputes** (every 120 ticks, i.e. every ~0.5–2 s at speed). Split fills/outlines
   from per-lot vacant dressing and park entities; key vacant dressing on its
   low/high bucket, parks on tier + polygon hash.
7. **Hot spots that will not scale** (nd_sim): O(B²) market matching per hour (full
   seller scan per buyer × input); linear `walk.nearest` and `nearest_street` per trip;
   ~5 O(citizens) + ~4 O(buildings) statistics scans *every tick*; O(L_old × L_new) lot
   re-matching; `nearest_road` per edge per recursion level in `subdivide` (1 km²
   zone over a 7k-segment grid: 1.26 s). One uniform grid index per rebuild (segments,
   lots, walk nodes), a per-resource seller index per hour, stats moved to the 10-tick
   economy update.
8. **Triangle budget.** Every flat detail (window frame, pane, graffiti stroke, LED cell,
   bay line) is a closed 12-tri box; `wire()` is 96 tris with ten hidden caps; caps and
   quads share no vertices. A 30-floor BalconySlab is 18k tris. Add a 2-tri `decal`
   primitive, share cap ring vertices, build wires as ring strips, and add a per-style
   triangle budget assertion to the sweep (vacant.rs already has one).

### Medium

9. `road_connected` for stations/yards depends on **an arbitrary lot's access node**
   (first usable zoned lot in the SlotMap). Compute road components once per rebuild.
10. **Save/load fragility**: `WorldConfig` lacks struct-level `#[serde(default)]`, so a
    new config field breaks every save; no schema version on `World`; `after_load`
    re-plans parks although layouts are serialised; no validation of dangling
    `home`/`work`/`inside`/`parked_at` ids after load. `ToolState` is not reset on load,
    there is one hard-coded save slot, and Load silently discards the running city.
11. **Economic balance**: wages are credited to citizens from thin air while businesses
    pay a separate 60% bill; rent vanishes; no credit floor on `savings` (consumer debt
    becomes money printing); closed venues still take cover charges. Document the
    sources/sinks (a `Treasury` ledger) and add an overdraft refusal.
12. **Containment test is weaker than DESIGN.md claims**: the sweep uses the lot AABB
    with a 4 m allowance (60/90 m for rail), tolerates 2 m below ground, never exercises
    a real `RailLink` on either side, nor a two-lot park layout. Freight yard sidings
    extend 34–50 m past the platform through whatever neighbours the railway; park
    features draw beyond their radius with no lot check.
13. `update_daylight` **rewrites the shared Glow material every frame**, re-uploading it
    and invalidating batches for every glow mesh. Quantise and write on change.
14. **Hot-reloaded palette can panic the renderer**: `pal.concrete[0]`,
    `metal_paint[1]`, `window_colors[0]` etc. are indexed directly in ~40 places;
    `random_bool(p)` is called with unclamped rule inputs that can exceed 1.
15. `signal_groups`/`signal_posts` in `street.rs` are quadratic (intersections ×
    connectors × all lanes) and iterate a `HashMap`, the only non-determinism found in
    procgen.
16. **`G` seeds a demo district with no confirmation**, irreversible from the project
    undo stack, and fires in walk mode too.
17. `pointer_over_ui` is one frame stale (panel runs in `PostUpdate`, tools in
    `Update`); Delete and Enter are not gated on it.
18. **Duplication**: truck dispatch ×3, boarding/alighting ×2, walk-to-stop ×2, special-
    building constructors ×4, lay-off code ×3 with a magic `income = 40`, two skill
    formulas, nine copies of the front-face + `fence_poly` idiom, two railway drawers,
    two `point_along`, two car drawers, four rejection samplers, three polygon offsets
    with different mitre caps. A `nd_sim::geom` module and a few `parts.rs` helpers would
    remove most of it.
19. **Architecture**: `world.rs` is 4.3k lines and `impl World` owns planning, market,
    construction, trips, parking, buses, trains and power; stats live in three places;
    `ui.rs` is one 620-line system with the citizen inspector duplicated in the person
    pane. Split by concern without behaviour change (`world/{plan,market,construction,
    trips,transit,rail,power}.rs`, `ui/{panel,tools,inspector,person,hover}.rs`).
20. Mixed lot-local and world coordinates in procgen (`Ctx::inside` local vs
    `Park::inside` world, `world_frame()` hack, `Face` struct-literal overrides whose
    `a/b` disagree with `tangent/out`).
21. Traffic: bus length ignored in next-lane lookahead; a bus that hits `TRIP_TIMEOUT`
    teleports with its riders; every approach gets a green phase even one-way exits;
    walk A* heuristic is inadmissible with the 0.92 park-edge factor.
22. `plan_trip` never checks that the ride goes forward round the loop.
23. Footfall is counted at planning time and for sub-legs, so one journey counts two or
    three times and abandoned trips count.
24. Tick budget hides slowdown: a heavy tick (daily update, land value) still runs to
    completion in one frame, and the accumulator is silently clamped. Show effective
    speed in the HUD; spread periodic work across ticks.
25. Per-frame full `Plan` clones in the tools (`effective_plan()` up to three times a
    frame while Zone/Edit/Bulldoze is active).

### Low

26. `extrude`/`frustum`/`ground_polygon` assume convex footprints (centroid fan);
    0.03% of triangles in the sweep have winding disagreeing with their normal.
27. UV seams on rotated slabs follow world axes; brick texture has 13 courses per tile
    (parity repeats at seams); normal-map strength depends on texture size.
28. `scatter_nature` RNG stream depends on the caller's `clear()` predicate, so one new
    road re-rolls every rock downstream in scan order.
29. `facility_lot` falls back to `SegmentId(0)` (the trunk) as a street-less station's
    front segment.
30. Dead code and warnings: `TrainTag`/`BuildingTag`/`CitizenTag`/`CarTag` payloads
    never read, `Unit.trailer`, `let _ = …` leftovers in rail.rs/transit.rs/parking.rs/
    mod.rs, `CountAndList` trait, `BuildingStyle` alias, misplaced doc comments, `glam`
    listed in `nd_app` but unused, empty workspace `repository`.
31. Docs drift: README does not list `O`, `G`, `F6`, Backspace or the letter tool
    aliases; `docs/IDEAS.md` still lists shipped features (projects with undo, bulldoze,
    one-way streets, rail freight, parking) as unscheduled; DESIGN roadmap lists v0.16
    before v0.15.

---

## 4. Test gaps worth closing first

- Determinism: same seed + same script → identical `to_ron()` after N ticks (pillar #4,
  untested).
- Save/load with trucks, buses and riders in flight; behaviour identical before/after.
- All world tests use `IdleMind`, so everyone starves and emigrates around day 6; the
  redevelopment test's closing assertions are satisfied by starvation, not rent. Pin
  satiety or use `RuleBasedMind`.
- No tests in `plan.rs`, `land_value.rs`, `transit.rs`, `rail.rs`, `street.rs`.
- Zoning: partial-overlap zones, self-intersecting zones.
- Procgen: cap orientation / winding test, per-style triangle budget, `Extras` paths
  (`powered < 1`, `parking`, `depot`, real `park`, real `rail` on both sides).
- Scale benchmark: 2,000 citizens × 1 day under a time budget.

---

## 5. Suggested order of work

1. Open items 1–3 (overlapping lots, trains on rebuild, stranded rail imports): small,
   high impact, each with a regression test.
2. Item 4 (time scale) — a design decision more than code; settle it before tuning
   anything else in the economy.
3. Items 5, 6, 8, 13 (renderer scaling) together, since chunk batching changes how
   buildings, vacant lots and parks are synced.
4. Item 7 (grid indices) before the city grows past a few hundred buildings.
5. Items 10–11 and the test gaps, then the structural splits (19) as a no-behaviour-
   change refactor when the UI rework starts.

---

## 6. Status after the second pass (five parallel branches, merged October 2026)

Every open item in section 3 was addressed. Branches: `review/sim-geometry`, `review/sim-world`,
`review/procgen`, `review/app-render`, `review/app-ui`. Tests: 57 → 108 (90 nd_sim, 18 nd_procgen).

| # | Status | How |
|---|---|---|
| 1 | Fixed | Newer zones are subtracted from older ones before subdivision. |
| 2 | Fixed | Rebuild clears trains and strands riders; stops are looked up with `get`. |
| 3 | Fixed | Disconnected or idle yards drain their imports by truck; site parts always go by truck. |
| 4 | Fixed | `traffic_seconds_per_tick` 4.0, speeds derived from m/s via `metres_per_tick`; a 300 m commute at 08:00 arrives before 09:00. |
| 5 | Fixed | Tangents from procgen, 128 m chunk batching per surface, render-world-only meshes, shadow flags. Vertex colours stay Float32x4 (Bevy requires it). |
| 6 | Fixed | Lot fills, parks and vacant dressing are three syncs keyed on what they draw. |
| 7 | Fixed | Uniform grids for segments, lots and walk nodes; hourly seller index; stats every 10 ticks. A* generation-stamp buffers not done (not needed yet). |
| 8 | Fixed | Decals, shared cap rings, ring-strip wires: mean 2,948 → 1,730 tris per building; per-style budget asserted. |
| 9 | Fixed | Union-find road components. |
| 10 | Fixed | `WorldConfig` defaults, `World.version`, `validate()` on load; timestamped slots, Load confirm, autosave at dawn. |
| 11 | Fixed | Overdraft floor, closed venues skipped, named treasury ledger. |
| 12 | Fixed | Sweep measures against the lot polygon with 1 m allowance, real `RailLink` both sides, multi-lot parks. `vacant.rs` sweep still uses the AABB. |
| 13 | Fixed | Glow material written only when the quantised value changes. |
| 14 | Fixed | Palette accessors, `validate()`, clamped probabilities. |
| 15 | Fixed | Connector map built once, `BTreeMap` grouping, determinism test. |
| 16 | Fixed | `G` seeds only on an empty map in orbit mode; the panel button adds it to a built city. |
| 17 | Fixed | `pointer_over_ui` computed in `PreUpdate`; `keyboard_captured` gates every hotkey. |
| 18 | Fixed | `dispatch_truck`, `board_waiting`, `place_special`, `nd_sim::geom`, shared procgen parts. |
| 19 | Partly | `ui/` split done; `world.rs` module split in progress on `review/world-split`. |
| 20–30 | Fixed | See the branch commit messages; `cargo check` is warning-free. |

Known leftovers: A* scratch buffers, the vacant sweep's AABB test, and a chunk regenerates
all its buildings on one change (no per-building geometry cache).
