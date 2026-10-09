# Code review: `nd_app` and `nd_minds` (October 2026, main @ 8e68a3e)

**Scope:**
- `crates/nd_app/src`: render.rs plus render/*, remote.rs plus remote/*, tools.rs, ui/*, the selection and overlay modules, sim.rs, camera.rs, main.rs, crash.rs, memory.rs and committee.rs.
- `crates/nd_minds`: the whole crate.

**Method:**
- This was a read-only review. Four parallel passes (render; remote and minds; tools and overlays; UI) were consolidated here.
- High findings and the ordering claims were re-checked against the source by the consolidating reviewer.
- Line numbers are as of 8e68a3e.
- "Suspected" marks a finding that was read but not fully traced.
- **Phase 2a (branch `fix/review-app`):** bug fixes are marked **FIXED <commit>** or **PARTLY FIXED** in the tables below; unmarked items are open (mostly refactors for phase 2b).

**Baseline:**
- `cargo check -p nd_app -p nd_minds`: clean. There are no warnings in either crate; the only warning is one in nd_procgen.
- Only one `#[allow(dead_code)]` exists (`ui/social.rs:21`, used by a font test). Because every module is private to the binary, rustc would flag unused items. **Dead code is therefore low:** what remains is over-wide `pub` visibility, unreachable match arms and dead parameters, not unused functions.

**Severity scale:**
- **H**: a user-visible bug, data loss, a security issue or a large per-frame cost.
- **M**: a real bug in an edge case, or a measurable cost.
- **L**: hygiene.

---

## Top 20 actions

### (a) Bug fixes

| # | Sev | Where | Action |
|---|---|---|---|
| 1 | H | `tools.rs:1146-1152`, `nd_sim/src/plan.rs:478` | **Undoing a control-point drag destroys history.** The drag calls `set_gesture(.., release)`, which amends while the button is held and pushes on release. `Project::amend` overwrites the last step when that step touched the same gesture. So "draw road, drag a point, Ctrl+Z" leaves the moved road, and a second Ctrl+Z deletes the road; the original shape is gone. **Fix:** push on the first drag frame and amend on every later frame, including release (`drag_started` in `ToolState`). Add a draw, drag, undo test. **FIXED 7c7a3f8 (drag pushes its own undo step on the first frame that moves; test `tools::drag_tests`)** |
| 2 | H | `crash.rs:67`, `memory.rs:53,92` | **`--features heap-count` cannot compile:** there are two `#[global_allocator]`s. **Fix:** fold the counters into `CrashAlloc` behind `cfg(feature)`. Add `cargo check --features heap-count` to the checks. **FIXED cbe3db0 (`cargo check -p nd_app --features heap-count` builds)** |
| 3 | H | `.gitignore`, `nd_minds/src/committee/settings.rs:8,140-154` | **`committee_settings.ron` can hold a pasted Claude API key in plaintext in the working directory**, which is the repo root under `cargo run`, and **it is not git-ignored** (`git check-ignore` confirms). **Fix:** add `/committee_settings.ron` to `.gitignore` now. Later, store the key per user (`%APPDATA%`) or in the OS keyring, never in the RON. **FIXED on main 9ee6376 (git-ignored); the per-user key store is still open** |
| 4 | H | `camera.rs:524`, `flags.rs:104` | **F8 is bound twice.** It is "jump to a random mid-tier building" and also "Flag for Claude". The camera jumps first, so every F8 flag records the wrong view. **Fix:** move the F7/F8/F9 jumps to Shift+F7/F8/F9, or drop the F8 jump. **FIXED b76d628 (Shift+F8 jumps; the flag keeps the view from when its box opened)** |
| 5 | H | `ui/social.rs:291-296` | **Speech bubbles cycle forever:** 4 s shown, 1 s gone, then shown again. `retain` drops a talk after 5 s and the next frame re-inserts it with a fresh `now`, because it is still among the last 200 talks. **Fix:** retain by serial (keep the entries still inside the 200-talk window), not by age. **FIXED 57bd9fb** |
| 6 | M | `tools.rs:729` (`draw_gizmos` in `SimSet::Input`) | **Selection outlines, train overlays, bus-line rings and rage rings are drawn before `step_sim`.** They use last frame's `frac` and the pre-tick state, while meshes, marks, ferry overlays and the follow camera use the post-tick state. At speed, the gold box trails its car. **Fix:** keep `tool_input` in Input and move `draw_gizmos` after `SimSet::Step`, ideally into an `OverlaySet` (see section 2.1). **FIXED 5d988ef (`draw_gizmos` in Sync; the OverlaySet is phase 2b)** |
| 7 | M | `render.rs:3175-3212` (`sync_ships`) | **No `full_rebuild` handling.** After a load, reused `ShipId`s keep the old ship's entity and its kind mesh. **Fix:** drain and despawn on `full_rebuild`, as `sync_bots` does at `render.rs:2627`. Better, use the central reset in item 16. **FIXED 7c7a3f8 (`render::reconcile`, keyed on full_rebuild and the ship kind; test `render::reconcile_tests`)** |
| 8 | M | `remote.rs:481-547`; `ui/inspector.rs:394,449`; `ui/lines.rs:188` | **Destructive actions with weak confirmation.** Remote `load`, `new_game` and `generate_city` replace the city with no `confirm`; only `revert` asks for one (`remote/identity.rs:87`). UI "armed" two-click confirmations are kept in egui temp memory keyed by a slotmap id, so after a load a building or line with the same id comes up already armed. **Fix:** require `confirm` when `saves.dirty`. Salt armed ids with `sim.generation`. **FIXED 7c7a3f8, 184c99b (confirm when `Sim::unsaved_play`), e0783b0 (armed ids salted with the generation)** |
| 9 | M | `committee.rs:148-153`; `nd_minds/src/committee/driver.rs:529-544`; `http.rs:64-69` | **Three committee correctness bugs:** <br>1. A multi-day `step` or frame runs `committee_daily` once, so days are skipped. <br>2. `Driver::run` drops voice and diary replies, which then stay `pending` forever. <br>3. The serial HTTP worker still sends turns the courier has already timed out, which costs money. <br>**Fix:** <br>1. Loop over the skipped days. <br>2. Return unclaimed deliveries to `voices.take`. <br>3. Share a cancelled-ids set or deadline with the worker. **FIXED 6555cee (1: `committee::daily` per step chunk; 2: `Driver::run` returns the others; 3: `CommitteeBackend::cancel`)** |
| 10 | M | `remote/view.rs:28-34,50-55`; `remote.rs:207-213` | **Remote input hardening:** <br>- `opt_f32` lets `inf` through: `grant {amount: 1e39}` puts an infinite treasury into the save. <br>- `key_from` does not range-check, so `"42:2"` aliases `42:3`. <br>- A panic in `handle` leaks an in-flight slot; after 16 panics every request gets a 503. <br>**Fix:** reject non-finite values; validate `idx`, `ver` and that `ver` is odd; release the slot with a drop guard. **FIXED 91c0184** |

### (b) Behaviour-preserving refactors

| # | Sev | Where | Action |
|---|---|---|---|
| 11 | H | `render.rs:3259-3292` (`lift_movers`) | **On relief maps, every `Grounded`, citizen, car and train entity gets `surface_height_near` and a Transform write every frame.** That includes parked cars, clamps, cabs and rubble, which never move. The write dirties transform propagation for thousands of static entities. **Fix:** lift static things once (`Added<Grounded>` plus a `Static` marker), and use `set_if_neq` elsewhere. |
| 12 | H | `render.rs:1396-1404`, `1557-1562`, `1615-1626`, `3100-3131`; `render/far.rs:44-81`; `render/mapview.rs:173-196` | **Signature hashing over the whole world every frame:** lots, parks, vacant land plus land value, the landscape signature (all buildings plus `format!("{:?}")`), `far_key` (all segments twice) and the map signature. **Fix:** gate each on sim version counters (`roads.version`, `parcels.version`, a lots/buildings revision, `land_value.version`). Add the missing counters to nd_sim, then hash only when a counter moves. |
| 13 | M | `render.rs:1096`, `2821`, `2519` | **Rebuild-every-frame traps:** <br>- `sync_lane_arrows` rebuilds every frame when a network has no arrows (`!existing.is_empty()` in the skip test). <br>- `sync_transit` rebuilds whenever routes exist without live stops. <br>- `sync_parked_cars` rescans every vehicle each tick. <br>**Fix:** key them on version counters only. |
| 14 | M | `ui/history.rs:236`, `ui/saves.rs:187`, `ui/title.rs:75-93` | **Disk I/O every frame:** the history window reads `read_dir` plus every JSON sidecar, and the title screen and saves panel call `fs::metadata` per card. **Fix:** cache the listing in `Saves`, keyed on the save generation, and keep the mtime from the scan. |
| 15 | M | `render/crossings.rs:177`, `haze.rs:282`, `grime.rs:287`, `climate.rs:308` (suspected) | **Material swap ping-pong under the heat and focus views:** these compare against the original material instead of `SwapOriginal`, so they rewrite the material every frame and the swapper swaps it back. **Fix:** compare the way `render.rs:1147` does: `heat_original.map_or(&mat.0, \|h\| &h.0)`. |
| 16 | M | `remote.rs:503-509,523-529`, `remote/identity.rs:96-103`, `ui/newgame.rs:184`, `ui/saves.rs:422`; `render.rs:1775-1797` | **City-replacement resets are copied four times, and the copies have drifted.** They also wipe favourites and tool preferences (`ToolState::default()`). **Fix:** one `on_city_replaced` system keyed on `sim.generation`. It resets the Selection, the id fields of `ToolState`, `EffectivePlan`, follow and flight, and despawns every entity with a `CityScoped` marker. Keep favourites in `World` (`#[serde(default)]`). **PARTLY FIXED 7c7a3f8: one `city_reset` system keyed on `Sim::generation` replaces the four copies and keeps the tool preferences. Not done: the `CityScoped` despawn marker (renderers still reset on `full_rebuild`) and favourites in `World` (an nd_sim field)** |
| 17 | M | `remote.rs:3016-3162` vs `execute` `:425-1038`; `docs/REMOTE.md` | **`help` and REMOTE.md have drifted from the dispatcher.** Not in help: `tow_car`, `pressure`, and `bulldoze {building}`. About 19 aliases are undocumented. `contaminate` and `paint_mural` are missing from REMOTE.md. **Fix:** the command registry in section 3.2, with help generated from it, plus a uniqueness and coverage unit test. **FIXED 91c0184 (one `ALIASES` table feeds help; `help_matches_the_dispatcher` reads the dispatch arms; the full registry is phase 2b)** |
| 18 | M | `remote.rs:1320`; `remote.rs:2005-2190`, `remote/list.rs:28`; `remote.rs:173-194,323` | **Remote API costs on the main thread:** <br>- `status` runs the full `report::alerts` scan on every poll. <br>- `find` and `list` build JSON for every match before applying `limit`, and `find`'s `limit` is uncapped. <br>- `remote_commands` borrows about 25 resources every frame even with an empty inbox. <br>**Fix:** cache alerts per sim hour; sort ids first and serialise only the survivors; add `run_if(inbox non-empty)`. |

### (c) Bigger restructures

| # | Sev | Where | Action |
|---|---|---|---|
| 19 | M | `tools.rs:1618-2373`, `build_highlight.rs`, `ferry_select.rs`, `toll_select.rs`, `train_select.rs`, `road_select.rs`, `marks.rs`, `stub_marks.rs`, `render/{plot_hover,layers,mapview,survey}.rs` | **Overlay framework** (section 2.1): an `OverlaySet { Validate, Build, Draw }` after `SimSet::Step`, a `MeshOverlay` trait plus a generic plugin, one `SelectionDraw` dispatcher that matches every `Selection` variant exhaustively, a `validate_selection` system, and shared terrain-aware shape builders. It fixes items 6 and 16 and B11, B12, B16 and B17 in one place, and removes about 250-300 lines. **REFACTORED 170677e, 8b08b48, daf9860 (`overlay/`, `selection.rs`; behaviour kept, so B16, B17, L8 are not fixed yet)** |
| 20 | M | `render.rs` (3.2k), `remote.rs` (3.2k), `tools.rs` (2.5k), `ui/inspector.rs` (1.2k) | **Split the four oversized files** along the concrete line ranges in section 3, starting with `RenderSet` sub-sets and per-feature render resources in place of one `RenderState` behind an 18-system `.chain()`. That chain serialises the whole static pipeline. |

---

## 1. Correctness

### 1.1 Bugs and panics

| ID | Sev | File:line | Problem | Fix |
|---|---|---|---|---|
| B1 | H | `tools.rs:1146-1152` | Undoing a drag destroys history (Top 20 #1). | See #1. **FIXED 7c7a3f8** |
| B2 | H | `ui/social.rs:291-296` | Bubbles re-show forever (#5). | See #5. **FIXED 57bd9fb** |
| B3 | H | `camera.rs:524` / `flags.rs:104` | F8 is bound twice (#4). | See #4. **FIXED b76d628** |
| B4 | M | `render.rs:3175` | `sync_ships` ignores `full_rebuild` (#7). | See #7. **FIXED 7c7a3f8** |
| B5 | M | `ui/lines.rs:47-62` | The stop-name cache is keyed on `transit.version`, which is `#[serde(skip)]` and so 0 after every load, plus the route count. City B can show city A's "by <building>" stop names. | Add `sim.generation` to the key, as `directory.rs` does. **FIXED e0783b0** |
| B6 | M | `render/emergency.rs:336`, `render/holo.rs:426-440` | Emergency unit bodies use `y = 0.05` plus `lift_movers`, not `car_height_at`. They carry no `InTunnel`, so a fire engine under a viaduct or in a tunnel is drawn on the top surface and ignores the Underground view (suspected visually). | Use `car_height_at`, add `SelfLifted` and `InTunnel`, and include `EmergencyTag` in `apply_depth`. **FIXED 0e7f729** |
| B7 | M | `ui/economy.rs:127-147` | The net bar's `scale` is the largest single ledger line, but `net` is a sum, so the bar overflows its 360 px track. | `scale = scale.max(net.abs())`, or clamp `len`. **FIXED 7e5b705** |
| B8 | M | `ui/inspector.rs:396-401` | When the inspector's Bulldoze gets `Err`, nothing is shown and the armed state is already cleared. | Write the error to `flags.message`, as `tools.rs:1250` does. **FIXED e0783b0** |
| B9 | M | `tools.rs:2301-2308` (`lot_outline`), callers `:2004-2212`, `marks.rs:116,118`; `tools.rs:2379` | Selection, owned-lot and Buildings-tool preview outlines use absolute `y` with no `surface_height`, so they sit under the terrain on relief maps. | A terrain-aware `lift` in the shared shapes module. **FIXED a47a393 (`lot_outline`)** |
| B10 | M | `tools.rs:1925` vs `:2431` vs `nd_sim/world/trains.rs:43` | Three different rules decide whether a rail piece is red: the Rail tool (any building), the Buildings tool (only special ones) and the sim (takes over ordinary lots). | Colour both previews from `World::piece_verdict` (`nd_sim/world/water.rs:979`). **FIXED 8806b31** |
| B11 | M | `district_tool.rs:132`, `tools.rs:591` | A right-drag with the District brush erases and orbits the camera at once, because `captures_right_drag` only knows the Zone tool. | Include `Tool::District` in brush mode. **FIXED 7e5b705** |
| B12 | M (likely) | `ui/toolbar.rs:232`, `ui/road.rs:94` | Implement from the toolbar drops a road selection: the road card sees no segment before `keep_on_rebuild` can re-point it. Ctrl+Enter works. | Make re-pointing a function and call it from the road card and from `validate_selection`. **FIXED a47a393** |
| B13 | M | `remote.rs:2477-2543` vs `tools.rs:1270,1511` | Remote `select` does not behave like a click. It never sets `route_sel` for a bus and never maps a unit's car to `Selection::Unit`. | Use one `apply_pick(sim, tools, sel)` for both. **FIXED a47a393** |
| B14 | M | `ui/inspector.rs:47-76` | Four unreachable arms (None, Citizen, Toll, Road), excluded by the guard at line 16. The Road arm discards `road::show`'s message. | Delete them and make `show` private. |
| B15 | M | `committee.rs:136-139`, `remote.rs:453` | After a reload with a meeting in session, `paused_before = Some(false)`, so the city always unpauses when the meeting adjourns. Remote `step` and `pause` ignore the meeting pause. | Persist `paused_before` with the meeting; make `step` and `pause` warn while a meeting is on. **PARTLY FIXED 6555cee (remote `step` stops when a meeting is called; persisting `paused_before` needs an nd_sim field)** |
| B16 | L | `marks.rs:107-108` | The bot's heading is interpolated linearly, so the outline spins at ±π. | Use the shared `lerp_angle`, which has three copies today: `camera.rs:298`, `render.rs:2140` and the bot code. |
| B17 | L | `ferry_select.rs:327-394` | The ship outline is drawn around the camera when you follow a ship from its deck. | Check `pov_in_vehicle`. |
| B18 | L | `render/layers.rs:378-386` | Vehicle markers are not despawned on rebuild, so a `VehicleMarker(CarId)` can attach to a different car after a load. They also use the raw heading, not the interpolated pose. | Despawn on rebuild; use `pose_at(f)`. **FIXED a47a393** |
| B19 | L | `tools.rs:993,1019` | A middle click is not guarded by `pointer_over_ui`, so middle-clicking a panel clears the remote pins. | Add the guard. **FIXED 7e5b705** |
| B20 | L | `tools.rs:920-924` | Undo or redo during a brush stroke leaves the stroke's counts, so the release status is wrong. | Clear `tools.stroke` on undo and redo. **FIXED 7e5b705** |
| B21 | L | `district_tool.rs:162` vs `tools.rs:927` | Right Ctrl+Enter implements the project and closes the district polygon at once. | Check both Ctrl keys in `district_tool`. **FIXED 7e5b705** |
| B22 | L | `toll_select.rs:12` | `footprint` checks the segment index is in range but not that the segment is alive. | `try_segment(..).filter(alive)`. **FIXED 7e5b705** |
| B23 | L | `fire_scene.rs:162-170` | `wrap` uses while-loops that never end for ±inf. | `rem_euclid`. **FIXED 7e5b705** |
| B24 | L | `render/survey.rs:161` | If the worker thread dies, `pending` never clears and the land layer never comes. | Handle `Disconnected`. **FIXED 7e5b705** |
| B25 | L | `render.rs:1232,1253` (suspected) | `pts[0]` and `last().unwrap()` on preview polylines panic if one is empty. | Slice patterns. **FIXED 7e5b705** |
| B26 | L | `render.rs:2330,2550`, `render/air.rs:125`, `render/taxis.rs:74` | `vr.finish[class as usize]` indexes a 3-element Vec, so a new `VehicleClass` would panic. | An array sized by the enum, or `.get()`. **FIXED 7e5b705** |
| B27 | L | `ui/build.rs:768-775` | `elide(s, 0)` underflows. The only caller passes 14, so it is latent. | `saturating_sub`. **FIXED 7e5b705** |
| B28 | L | `ui/skyline.rs:26-32`, `ui/estate.rs:11-21` | `money` shows "$0M" below $500k and "$-1M" for negative values. | The shared `fmt::money` (section 2.3). |
| B29 | L | `ui/people.rs:325-331` | The gift message always says "household", even when the gift went to savings. It duplicates `follow::gift_button`. | Use one `gift_message`. |
| B30 | L | `ui/title.rs:103-106`; `remote.rs:489,1352`, `remote/identity.rs:101` | A failed load still closes the title screen. Load and save success is detected by matching `flags.message` text, so a partial save failure ("slot copy failed") reports `ok`. | Make `load_now`, `save_now` and `revert` return `Result<String,String>`. **PARTLY FIXED 7c7a3f8 (`load_now` and `revert` return Result; a failed load keeps the title screen; `save_now` still reports by text)** |
| B31 | L | `flags.rs:52-71`, `remote.rs:2438` | Flag ids restart at 1 every run while the `.jsonl` keeps appending. Stored selections are not tied to a city generation, so `now` can describe a different object. | Seed `next` from the file's last id; store the generation in each flag. |
| B32 | L | `remote.rs:1335-1356` | `save {path}` writes only the `.ron`, with no meta sidecar and no thumbnail, unlike `save {name}`. | Route it through `write_save`. |
| B33 | L | `remote.rs:2742-2749`, `:783-855` | `camera` and `overlay` change state before they validate, so an error leaves partial state. | Parse first, then apply. |
| B34 | L | `remote/committee.rs:112`, `remote.rs:2001`, `remote/corp.rs:170`, `remote/nights.rs:182`, `remote/crossings.rs:55` | `as u32` truncates, and some fields are read without the checked `opt_*` parsers, so a wrongly typed field is silently ignored. | Use `u32::try_from` and the `opt_*` helpers. |
| B35 | L | `remote.rs:887` | `events {since: 0}` reports `dropped: false` even after the ring has rotated. | Compute `dropped` regardless of `since`. **FIXED 7e5b705** |
| B36 | L | `remote.rs:2912,2948` | `shot-<secs>` and `city-<secs>` default names collide within a second. | Millis plus a counter. **FIXED 7e5b705** |

### 1.2 System ordering and stale reads

| ID | Sev | File:line | Problem | Fix |
|---|---|---|---|---|
| O1 | M | `tools.rs:729` | Selection gizmos run before Step (Top 20 #6). | Move them to the `OverlaySet` after Step. **FIXED 5d988ef** |
| O2 | M | render.rs `sync_lots :1392`, `sync_overlay :2696`, `sync_paths :2738`; `render/layers.rs:286`; `render/survey.rs:145` | Hotkeys O, V, P, L and K are read inside Sync render systems, after `map_layers::keep_exclusive` (PreUpdate). For one frame two exclusive layers are both shown, and earlier systems in the chain see the toggle a frame late. | Move them into a `SimSet::Input` system that calls `map_layers::set_active`. **FIXED 6807178** |
| O3 | M | ui/* (`inspector.rs:57-87,158,200`; `train.rs:56`; `toll.rs:43`; `road.rs:42,94`; `person.rs:242`) | Checks that the selection still exists live only in the egui pass. Update-phase overlays, remote `inspect` and selected-detail, and the F8 flag snapshot can see a vanished car, or a unit already back in its station, for a frame. Nothing panics, because every consumer uses `.get()`. | One `validate_selection` system after Step. **FIXED 6807178 (`tools::validate_selection`)** |
| O4 | L | `render.rs:478` | `sun_cascades_follow_camera` has no ordering against `control_camera`, so it can be one frame late. | `.after(SimSet::Sync)`. |
| O5 | L | `render/focus.rs:133`, `render/heat.rs:229` | Daylight keys are reset in PostUpdate, so the frame where focus or heat ends renders with the wrong fog. | Restore the fog in `focus_fog`. |
| O6 | L | `flags.rs:85`, `metro_ux.rs:52` | These run in `EguiPrimaryContextPass` with no order relative to `ui::side_panel`, which consumes `want_flag` and drops keyboard focus. | Add `.after(crate::ui::side_panel)`, as `committee.rs:121` and `remote.rs:145` do. **FIXED 6807178** |

### 1.3 Selection enum coverage

`Selection` (`tools.rs:693-719`) has 12 variants: None, Citizen, Building, Car, Parked, Toll, Bot, Train, Metro, Road, Unit and Ship.

**Exhaustive matches:** `camera.rs:231-296` (`thing_pose`), `remote.rs:2351` (inspect), `remote.rs:2397` (`selected_detail`), `remote/view.rs:227-236` and `ui/inspector.rs:47`.

**`_ =>` arms that are legitimate:** `ferry_select.rs:262/329/399`, `marks.rs:86`, `ui/directory.rs:198/272` and `camera.rs:113`.

**Gaps:**
- **FIXED 0e7f729 (`render::lifts`, one exhaustive match):** **Render highlight/lift only knows `Citizen` and `Car`** (`render.rs:2118,2394`). A clicked emergency vehicle becomes `Unit`: its generic car is hidden (`emergency.rs:276-283`) and the unit body ignores selection, so a selected ambulance gets no lift or highlight. **Fix:** a `selected_entities(&Selection, &RenderMaps) -> SmallVec<Entity>` with no `_` arm.
- **Selection drawing is spread over five systems:** `tools::draw_selection`, `train_select::draw`, `ferry_select::draw`, `marks` (Bot), and fire_scene poses. A new variant has to be wired into several of them. The `SelectionDraw` dispatcher in section 2.1 makes the compiler enforce coverage. **REFACTORED 170677e (`overlay::selection::draw_selection`, no `_` arm)**
- **The selection is cleared on every city-replacement path** (`remote.rs:503,523`, `ui/newgame.rs:184`, `ui/saves.rs:422`), but by four copies of the code (#16).

### 1.4 Resource and entity leaks

`Mesh3d` handles are strong, so a despawn-and-rebuild loop causes churn, not leaks. No overlay module was found leaking meshes or materials. What does grow:

| ID | Sev | File:line | Problem | Fix |
|---|---|---|---|---|
| L1 | L | `render.rs:1886` | `sim.building_tops` is inserted into but never pruned, nor cleared on `full_rebuild` (`styles` is cleared). | Remove entries on `gone`; clear on rebuild. **FIXED a47a393** |
| L2 | L | `ui/saves.rs:146,186-198` | The thumbnail texture `pictures` map is never pruned, so pruned slots keep their GPU textures. A PNG that fails to decode is re-decoded every frame. | `retain` on rescan; remember failures. |
| L3 | L | `render/swap.rs:62-69` | The `derived`, `stand_ins` and `windows` caches are never evicted. Suspected: glow stand-ins miss later daylight changes. | Clear on a view change or a `glow_key` change. |
| L4 | L | `render/stunts.rs:203` | A new `Cuboid` mesh asset is added per crashed car. | Cache it in `StuntAssets`. |
| L5 | L | `render.rs:3182` | Ship meshes are cached forever and ignore `palette_changed`. | Rebuild on a palette change. |
| L6 | L | `ferry_select.rs:53-67` | The water network cache is a process-global `static Mutex`. It survives city changes, and its signature does not hash the preset or check that the docks still exist (suspected stale reuse). | Make it a Resource keyed by `(generation, sig)`. |
| L7 | L | `flags.rs:24,61-69`; `crash.rs:157-160` | The flag list and file grow without bound. Panics on the request and HTTP threads are logged as crashes, and rotation pushes out the real crash logs. | Cap or rotate; tag non-fatal threads. |
| L8 | L | `render/mapview.rs:211-232` (suspected) | `hide_for_map` hides every `Mesh3d` without `MapTag`, including build highlights, ferry bands and the plot hover. | Exempt the shared `OverlayPiece` marker. |

### 1.5 Remote API: what can change the user's game

**Posture (verified good):**
- It binds 127.0.0.1 only (`remote.rs:128`).
- `guard.rs:36-73` runs before the body is read. It checks the Host, refuses any Origin including `null`, checks `Sec-Fetch-Site`, and needs a JSON content type or `X-ND-Remote` on POST. GET serves only `/status` and `/help`.
- Bodies are capped at 1 MB and at most 16 requests run at once.
- `guard::confine` refuses `..`, UNC/device paths and wrong extensions, and canonicalises symlinks.
- No browser CSRF path was found.

**Gaps:**

| ID | Sev | File:line | Problem | Fix |
|---|---|---|---|---|
| R1 | H | `remote.rs:104,121-137`, `main.rs:78` | **The full mutating API is on by default in the release exe on a fixed port, with no authentication.** Any local process can wipe the city, overwrite saves, or switch to the paid backend and spend money (R3). | A per-run random token in `saves/.remote_token`, required in `X-ND-Remote`. Or `ND_REMOTE=readonly` driven by the registry's `effect` flag. Or default the API off in release unless a setting enables it. |
| R2 | M | `remote.rs:481-547` | `load`, `new_game`, `generate_city` and `seed_demo` need no confirm (#8). | Require `confirm` when the city has unsaved changes. **FIXED 7c7a3f8, 184c99b (`seed_demo` adds to the city, so it needs none)** |
| R3 | M | `remote/committee.rs:190-228,275-291` | The remote API can switch the committee to the Claude backend; `diary {refresh}`, `check_alerts` and `committee_meeting start` then make paid calls. There is no budget. `settings {test}` spawns a thread every call, each living up to 60 s (`committee.rs:91`). | Refuse remote backend switches unless the UI allowed them; add a daily call budget in `Courier::submit`; ignore `test` while one runs. **PARTLY FIXED a47a393 (backend fields need the player's leave in the window; one test at a time; no daily budget yet)** |
| R4 | M | `remote.rs:1331-1358,2946-2968,481-494` | `save`, `export` and `load` serialise or parse the whole world on the main thread. | Use the existing autosave writer thread (`ui/saves.rs:669`). |
| R5 | L | `remote/guard.rs:80-110` | Any file command may write into any of the three folders, so `export` can overwrite a save's `.meta.json` and `screenshot` a save's thumbnail. | Give each command its own root folder. |

**Mutating commands** (all behind the header guard only):
- **Time:** `pause`, `speed`, `step` (up to 100k ticks on the main thread).
- **City replacement:** `load`, `new_game`, `generate_city`, `seed_demo` (and `game_history revert`, which needs `confirm`).
- **File writes:** `save`, `export`, `screenshot`, `game_history mark/bookmark/unmark`, `flag`, `committee settings save`.
- **Building and planning:** `add_road`, `add_roundabout`, `add_interchange`, `add_zone`, `paint`, `bulldoze`, `project`, `place`, `intersection`, `lane_use`, `metro_line`, `subway`, `bus`, `district`, `industry`, `waterfront_parcels`, `rebuild_roads`, `repair_road`, `roads`, `skyline`, `estate`, `corps`, `campus`, `taxes`, `tolls`, `parking_price`, `tow_car`.
- **Money and developer tools:** `grant`, `gift`, `remediate`, `cleanup`, `contaminate`, `paint_mural`, `set_weather`, `walk_to`, `incident`, `race`, `rave`, `society`, `tourism`, `city_name`.
- **Committee (paid calls possible):** `committee *`, `committee_meeting *`, `diary`.
- **UI state:** `tool`, `camera`, `follow`, `zoom_to`, `select`, `favourite`, `overlay`, `lighting`, `heat_vision`, `graphics`, `window`, `city_data`, `directory`, `build_highlight`, `flags clear`.

### 1.6 Secrets (LLM backends)

**Verified good:**
- The key comes from `ANTHROPIC_API_KEY`, else from `committee_settings.ron` (`settings.rs:151-154`).
- `Debug` is redacted by hand for `CommitteeSettings` (`settings.rs:96-112`) and `Flavor` (`http.rs:28-35`).
- HTTP errors keep only the status and the server message, and transport errors only `kind()` (`http.rs:205-218`).
- Remote `settings` never takes or returns the key; it reports only its source.
- The UI field is a password field (`ui/committee.rs:571`).
- `crash.rs` writes no environment.
- HTTP calls run on a worker thread with a ureq timeout, and the courier has its own deadline.
- Tests cover the redaction.

**Problems:**
- **S-1 (H):** the RON file is not git-ignored (#3). **FIXED on main 9ee6376**
- **S-2 (L):** `ui/committee.rs:536` clones the settings, key included, every frame the tab is open. It is harmless, but borrow instead (this also helps P-UI4).
- **S-3 (L):** the OOM path in `crash.rs:76-91` formats, captures a backtrace and does file I/O from inside `GlobalAlloc::alloc`. That can deadlock on std's backtrace lock (suspected). **Fix:** write a preformatted static message, and capture backtraces only in the panic hook.

---

## 2. Duplication

### 2.1 A common overlay framework

**REFACTORED 170677e, 8b08b48 (`overlay/{mod,shapes,selection,mesh,routes}.rs`: OverlaySet, OverlayPiece, OverlayMaterials, MeshOverlay + MeshOverlayPlugin for build_highlight, plot_hover and the ferry bands; mapview, layers, survey and footbridges not moved; the fixes listed under Gains are left for a later phase).**

**The pattern each overlay module repeats by hand:** a cache resource holding (key, entities, materials); read `Sim`, `Selection` and `ToolState`; compute a hash key; return early if it is unchanged and `!full_rebuild`; otherwise despawn, build `MeshData` and spawn with a lazily created unlit additive material. Gizmos are drawn separately. This holds for `build_highlight` (`:523-716`), `render/plot_hover` (`:298-361`), `ferry_select::sync_lines` (`:256-301`), `render/mapview::sync_map`, `render/layers::sync_layers`, `render/survey` and `junction_view` footbridges.

**Selection gizmos are spread out:** `tools::draw_selection` (`tools.rs:1970-2226`), `toll_select::draw`, `road_select::draw`, `train_select::draw`, `ferry_select::draw` (ship), `marks` (Bot and pins) and the `fire_scene` unit outline. They are scheduled in Input, Sync or after Sync, depending on the module.

**Proposal: `crates/nd_app/src/overlay/`.**

```rust
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum OverlaySet { Validate, Build, Draw }   // Validate after SimSet::Step; Build/Draw in Sync after lift_movers
#[derive(Component)] pub struct OverlayPiece;   // shared despawn marker; mapview/swap/focus exempt it
pub struct OverlayCtx<'a> { pub sim: &'a Sim, pub sel: Selection, pub frac: f32, pub t: f32, pub pov: bool, pub under: bool }
pub trait MeshOverlay: Send + Sync + 'static {
    type Key: PartialEq + Send + Sync + 'static;
    type Param: ReadOnlySystemParam;
    fn key(p: &SystemParamItem<Self::Param>, cx: &OverlayCtx) -> Option<Self::Key>;   // None = nothing shown
    fn build(p: &SystemParamItem<Self::Param>, cx: &OverlayCtx, k: &Self::Key, out: &mut Pieces);
    fn restyle(_k: &Self::Key, _ents: &[Entity], _cmds: &mut Commands) {}           // e.g. ferry bright/dim
}
pub struct MeshOverlayPlugin<T: MeshOverlay>(PhantomData<T>);
// registers OverlayState<T>{ key, generation, ents } and one generic sync<T>():
//   if key == state.key && generation unchanged && !full_rebuild { return }
//   despawn state.ents; T::build; spawn with OverlayMaterials + OverlayPiece
#[derive(Resource)] pub struct OverlayMaterials { additive: [Handle<StandardMaterial>; 3], pulse: Handle<StandardMaterial> }
pub fn draw_selection(g: &mut Gizmos, cx: &OverlayCtx) { match cx.sel { /* every variant, no `_` */ } }
// overlay/shapes.rs: ribbon, outline_band, light_wall, fill, vehicle_box, person_box,
//   lot_outline (terrain-aware), route_ends, ring, closed(poly), lerp_angle, GOLD, FLAT
```

**Modules that would move:**
- **`MeshOverlay`:** build_highlight, plot_hover, ferry bands (selection-driven `restyle`), mapview, and the layers grids.
- **The `draw_selection` dispatcher:** all selection gizmos.
- **`OverlaySet::Validate`:** `validate_selection`, the road re-pointing, and the reset on a new generation.

**Gains:**
- It fixes O1, O3, B9, B12, B16, B17 and L8.
- It removes about 250-300 lines: about 80 in build_highlight, 75 in plot_hover, 50 in ferry, 25 in mapview and layers, 15 from the car-outline copies, 12 from the `vehicle_outline` copy in `render/layers.rs:462-472`, and 20 from the GOLD and flat-quaternion literals (gold `(3.2,2.4,0.7)` appears 6 times; the flat quaternion 14 times, with opposite signs in `district_tool.rs:208` and `metro_ux.rs:188`).

### 2.2 Render helpers

**D-R1: "sync entities with a sim collection".** The same spawn-missing, despawn-gone, then pose loop appears in:
- render.rs: `sync_citizens` :2031, `sync_cars` :2318, `sync_bots` :2637, `sync_trains` :2770, `sync_ships` :3186
- `air.rs:120`, `subway::sync_metro_trains`, `districts::sync_tourists` :115, `emergency.rs:285`, `fire_scene` :135, `haze` :216, `grime` :224

**Helper:** `EntityMap<K>` with `reconcile(cmds, live)`, `spawn_missing(ids, f)` and `clear(cmds)`. It would also absorb about 25 despawn-a-list loops.

**D-R2: "keyed rebuild"**, about 25 systems. **Helper:** `RebuildGate<K>::due(&mut self, key, &Sim, &GameAssets) -> bool`, which folds in `full_rebuild`, `palette_changed` and the generation.

**D-R3: per-surface spawn with the glow special case**, in `render.rs:615,630,1066,1937`, solar, turbines and crossings. **Helper:** `surface_bundle(s, mesh, mats)`.

**D-R4:** `spawn_overlay` (`render.rs:650`) and `layers::spawn_layer` (`layers.rs:81`) are the same function.

**D-R5:** "day fraction or hour/24" appears 7 times (`render.rs:2243,2941`, `climate.rs:104`, `grime.rs:169`, `haze.rs:152`, `plot_hover.rs:344`, `solar.rs:75`). **Helpers:** `day_fraction(&Sim,&Lighting)` and `sun_elevation(t)`.

**D-R6:** `solar.rs` and `turbines.rs` sync are nearly line-for-line identical. **Helper:** a generic `sync_plant_farm<P: FarmKind>`, plus one plant index resource rebuilt on building events (this also fixes P-R2).

**D-R7: assets built more than once:**
- puff image and alpha-ramp materials three times (`haze.rs:132`, `grime.rs:110`, `climate.rs:224`)
- the water material twice (`render.rs:3149`, `far.rs:101`)
- the vehicle spawn (`ensure_materials` plus `finish[class]` plus body, lights and fx) five times: `sync_cars`, `sync_parked_cars`, `sync_bots`, `air.rs`, `taxis.rs`

**D-R8:** `air.rs:60-68` (`drawn_pads`) reruns the full building generator to read the pads. Record the pads in `sync_buildings` instead.

**D-R9:** the closed outline (`o.push(poly[0])`) appears 9 times in render.rs and 4 times in tools.rs, the latter with no length check. **Helper:** `closed(poly)`.

### 2.3 UI helpers

**New `ui/fmt.rs`** (pure, unit-testable):

| Helper | Replaces |
|---|---|
| `capitalise` | 7 copies: `hover.rs:207`, `inspector.rs:1097`, `road.rs:384`, `toll.rs:221`, `directory.rs:450`, `person.rs:252`, `pressure.rs:75` |
| `plural` | 33 inline `if n == 1` patterns and `road.rs:85` |
| `money` (sign-aware, k/M/B) | `estate.rs:11`, `skyline.rs:26`, `directory.rs:455`, and the ad-hoc formats in `topbar.rs:369`, `build.rs:762`, `follow.rs:80`, `people.rs:129` |
| `compact` | `pane::short` (moved here) |
| `signed` | `directory.rs:459` |
| `minutes` | `inspector.rs:1161` and `:1223`, which disagree ("2 h 05" vs "2 h 05 min"); also `toll.rs:150`, `emergencies.rs:57` |
| `day_time` | about 7 `/1440 + 1` sites |
| `days_text`, `elide`, `age_since(now)` | their single current copies |

**Move the colours into `theme.rs`:**
- GOOD green exists in three shades: (110,200,140) in about 9 files, (120,220,140) in 2, (120,230,150) in about 8.
- PLOT_BG (20,22,34,220) appears 8 times, PANEL_FILL 7, EDGE 3, SERVICE_BLUE 4, AMBER 5.
- GOLD, IVORY and WARN are written as literals in 7 places.
- `lerp(Color32)` is duplicated at `inspector.rs:993` and `concerns.rs:312`, and the copies round differently.
- The colour-array converters (`mod.rs:387` = `districts.rs:18`; `portrait.rs:17` = `person.rs:113`; plus `lines.rs:22`, `train.rs:24`, `legend.rs:15`) become `theme::rgb_f`, `rgb_u8` and `rgb_linear`.

**Cards and charts** (`pane.rs`):
- `tag` (identical at `inspector.rs:760` and `train.rs:45`), `figure`, `row_small`.
- `inspector_window(..)`: one shell for `inspector.rs:20-40`, `road.rs:59-74` and `toll.rs:24-41`. They share an id, but their default heights have drifted: unset, 820 and 780.
- `armed_button(ui, id, ..)` with a generation-salted id (#8).
- `hour_bars(..)`: `road.rs:343-382` and `toll.rs:171-219` are about 80% the same.
- `meter(ui, size, frac, col, marks)` for the one-value bars in `pressure.rs:35`, `economy.rs:334`, `detail.rs:98`, `road.rs:328` and `taxes.rs:210`.
- `signed_meter`, clamped (B7).
- `dashed_line`, merging `concerns.rs:197` and `people.rs:226`.

**Tables:** `directory::table` is the only virtualised sortable table. Reuse it if skyline, lines or tolls grow.

**Window title style:** two styles coexist: egui's title bar with `.open()`, and `pane::title_bar`. Settle on `pane::title_bar`.

### 2.4 Remote helpers

- **The city-reset block, four copies:** see #16.
- **The `window` command:** six guard arms re-parse and lowercase `name` (`remote.rs:944-1011`). Parse it once and match on an enum in `window.rs`.
- **Repeated code:** `SystemTime::now()...as_secs()` (`remote.rs:2912,2948,2955`, `flags.rs:59`; use `ui::saves::now_unix`), and the `Instant` plus `took_ms` timing, 6 times with integer and float variants. Use one `timed(|| ..)` helper.
- **Helpers in the wrong place:** `opt_strings` and `ms` (`remote.rs:2927`) and `str_filter`/`bool_filter` (`:1997`) belong in `view.rs`.
- **`help`** is split into four `json!` literals because of the macro recursion limit. A generated help removes this.

---

## 3. Structure

### 3.1 render.rs (3,292 lines) to `render/*`

**Facts:**
- `RenderPlugin::build` makes 45 `add_systems` calls and registers about 80 systems, plus about 10 more in the heat, look, mapview and thumbs sub-plugins.
- The only set is `SimSet::Sync`, held together by about 40 ad-hoc `.after`/`.before` edges against concrete functions.
- An 18-system `.chain()` (`:462-466`), almost all on `ResMut<RenderState>`, makes the static pipeline run serially.

**Plan:**
1. Introduce `RenderSet::{Reset, Static, Movers, Lift, Light}`.
2. Split `RenderState` (`:387-432`) into per-feature resources.
3. Replace the chain with set ordering: `Static → Lift`, `Movers → Lift`.

**Target files:**

| Lines | Target |
|---|---|
| 1-109 | `render.rs` (mods, plugin; the plugin restructured around `RenderSet`) |
| 166-262, 2684-2761 | `render/overlays.rs` |
| 264-283, 866-890, 2242-2247, 2910-3061 | `render/daylight.rs` (with `day_fraction`) |
| 285-314, 3214-3292 | `render/movers.rs`, `render/lift.rs` |
| 316-379, 692-864 | `render/materials.rs` |
| 548-678, 1166-1179, 2140-2150, 2902-2908 | `render/mesh_util.rs` |
| 681-1023 | `render/ground.rs` (terrain, scatter) |
| 1025-1164, 1978-2007, 2811-2900 | `render/streets.rs` (roads, arrows, signals, walk detail, transit, tolls) |
| 1181-1363 | `render/preview.rs`, or merged into `holo.rs` |
| 1365-1655 | `render/lots.rs` |
| 110-119, 1657-1976 | `render/buildings.rs` (`clear_full_rebuild` moves to `render/reset.rs` with the `CityScoped` reset) |
| 2009-2138 | merged into `render/people.rs` |
| 2152-2500, 2618-2682 | `render/vehicles.rs` |
| 2502-2616 | `render/parked.rs` |
| 2763-2809 | `render/trains.rs` |
| 3063-3212 | `render/landscape.rs` |

### 3.2 remote.rs (3,175 lines) to a command registry

```rust
pub enum Effect { Read, View, Game, Replace, Files, Dev, Paid }
pub struct Cmd { pub name: &'static str, pub aliases: &'static [&'static str], pub effect: Effect,
                 pub help: &'static str, pub run: fn(&Value, &mut Ctx) -> Result<Value, String> }
pub static COMMANDS: &[Cmd] = &[ /* per-module consts concatenated */ ];
```

**How `execute` changes:** it becomes a lookup in a `OnceLock<HashMap>`, then a check of `effect` against the mode (read-only, token, `confirm` for `Replace`, budget for `Paid`), then a call to `run`. `help` and the REMOTE.md table are generated from the registry.

**Tests:** names and aliases are unique; every entry has help; every `Replace` command refuses without `confirm`.

**Split:**

| Lines | Target |
|---|---|
| 1-147 | `remote.rs` (about 150 lines) |
| 149-171, 196-281 | `remote/server.rs` |
| 173-194, 349-413, 882-889 | `remote/events.rs` |
| 283-347 | `remote/ctx.rs` |
| 425-1038 | `remote/dispatch.rs` (the registry) |
| 1040-1107, 1245-1327, 2899-2909, 3164-3175 | `remote/status.rs` |
| 1109-1243, 1416-1631 | `remote/planning.rs` |
| 1329-1414, 2911-2968, plus `load` | `remote/files.rs` |
| 1633-1870 | `remote/place.rs` |
| 1872-1993 | `remote/bus.rs` |
| 1995-2192, 2228-2435, 2473-2546 | `remote/inspect.rs` |
| 2193-2226 | `remote/junction.rs` |
| 2436-2456 | `remote/flags.rs` |
| 2458-2471 | `remote/road.rs` |
| 2550-2737 | `remote/tool.rs` |
| 2739-2893 | `remote/camera.rs` |
| 2975-3014 | `remote/window.rs` |
| 3016-3162 | generated in `remote/help.rs` |

**Inline arms that leave `execute`:**
- time and city commands → `remote/time.rs`, `remote/city.rs`
- `grant`, `contaminate`, `paint_mural`, `walk_to`, `set_weather`, `rebuild_roads` → `remote/dev.rs`
- `overlay`, `lighting`, `heat_vision`, `graphics` → `remote/look.rs`
- the `window` and `city_data` arms → `remote/window.rs`

### 3.3 tools.rs (2,484 lines) to `tools/*`

**REFACTORED daf9860 (`tools/*`, `selection.rs`, `overlay/{selection,shapes,routes}.rs`; largest file `tools/state.rs`, 495 lines).**

| Lines | Target |
|---|---|
| 1-180 | `tools/kinds.rs` |
| 181-620 | `tools/state.rs` |
| 622-664 | `tools/zone.rs` |
| 666-690 | `tools/plan_cache.rs` |
| 692-719 | crate-level `selection.rs` (with `validate_selection` and `apply_pick`) |
| 721-756 | `tools/mod.rs` |
| 757-834 | `tools/edit.rs` |
| 836-978 | `tools/input.rs`, returning an `InputFrame` |
| 990-1470 (the per-tool arms) and the matching previews from `draw_gizmos` | `tools/{road,zone,edit,bulldoze,inspect,transit,rail,buildings}.rs`, each with `fn input(..)` and `fn preview(..)` |
| 1511-1616 | `tools/pick.rs` |
| bus lines and rage rings from `draw_gizmos` | `overlay/routes.rs` and render |
| 1970-2226 | `overlay/selection.rs` |
| 2228-2373 | `overlay/shapes.rs` |

After the split, the largest file is about 300 lines.

### 3.4 ui

- **`inspector.rs` (1,229 lines):** `inspector/{mod,building,vehicle,unit,bot}.rs`.
- **`build.rs`:** split the catalogue model (lines 1-450, pure and tested) from the window (lines 450-1000).
- **`saves.rs`:** `saves/io.rs` (scan, meta, write, load, revert, autosave) and `saves/panel.rs`.
- **`detail.rs`:** split by City Data tab into city, market, power and transit.

### 3.5 Plugin registration

`main.rs` holds a 12-plugin tuple plus a 5-plugin tuple. The comment notes the 15-element tuple limit. `train_select`, `toll_select` and `road_select` have no plugin: their systems are wired through `tools.rs:729` and direct calls.

**Proposal:** a few domain `PluginGroup`s, so main.rs adds about 4 groups and new modules register inside their group, never in main.rs:
- `CorePlugins`: assets, sim, crash, flags
- `WorldViewPlugins`: render, camera, map_layers, marks, the overlay framework (which takes build_highlight, stub_marks, ferry_select, metro_ux and the select modules)
- `PlayerPlugins`: tools, district_tool, ui, committee
- `RemotePlugin`

Also move `configure_sets(Update, (Input, Step, Sync).chain())` from `ToolsPlugin` (`tools.rs:728`) into `SimPlugin`, which owns `SimSet`. **REFACTORED daf9860**

### 3.6 Dead code and stale comments

**Dead code:**
- Unreachable inspector arms (B14).
- `detail.rs:232` `plant_table(.., wide)`: the only caller passes `true`, so the compact branch is dead.
- `road_select.rs:120-121` has a leftover `let _ = s;`.
- `inspector.rs:302` has an empty `if car.is_some() { }`.
- `theme.rs:190-196` `saved_scale` duplicates the parse in `load_settings`.

**Over-wide visibility:**
- About 26 UI `pub fn`s are used only in their own file.
- Render tags (`InTunnel`, `SelfLifted`, `BotTag`, `WayHeight`, `Grounded`, `CarTag`, `CitizenTag`, `BuildingTag`) and `offstage`, `plant_field` and `building_geometry` are `pub` or `pub(crate)` but used only in render.
- **Fix:** demote them to `pub(super)` or private.

**Stale or misplaced comments:**

| Where | Problem |
|---|---|
| `render.rs:33` | The emergency doc is on `mod thumbs`. |
| `render.rs:1653` | The `append_building` doc is on `plant_field`. |
| `render.rs:1375` | Says "two meshes"; there are three. |
| `render.rs:3218` | A parameter named `arrows` filters tolls. |
| `remote.rs:17-19` | The `[concerns]` doc is on `mod committee`. |
| `remote.rs:33-36` | The guard doc is on `mod grounds`. |
| `remote.rs:3062,3073,3133` | Empty `// [tag]` markers. |
| `ui/mod.rs:9-11` | The `[concerns]` doc is on `mod build`. |
| `render/nights.rs:227-231` | A stale 5-second rebuild comment and term. |
| `render/stunts.rs:79` | A local `despawn_all` shadows `super::despawn_all` with a different signature. |

---

## 4. Performance

High and medium items are in the Top 20: #11-#15 and #18. The rest:

| ID | Sev | File:line | Per-frame work | Fix |
|---|---|---|---|---|
| P-R1 | M | `render.rs:2070,2112` | `Visibility` is written unconditionally for every citizen, indoor ones included, which triggers visibility propagation. | `set_if_neq`; track the last state. |
| P-R2 | M | `solar.rs:81`, `turbines.rs:78`, `haze.rs:174`, `grime.rs:184` | All buildings are iterated every frame. `plant_cluster` (O(B)) runs twice per plant on each recheck, and `seen.contains` is O(n²). | A plant index resource (D-R6). |
| P-R3 | L | `render.rs:1812,1855` | Two O(n) building scans every frame in `sync_buildings`. | Gate on a revision. |
| P-R4 | L | `render.rs:1049,1119` | `closed_crosswalks()` is allocated and hashed twice per frame. | Compute once, or key on a walk version. |
| P-R5 | L | `render/holo.rs:275`, `render/plot_hover.rs:320`, `render.rs:3100` | `format!("{:?}")` used as a hash key every frame. | Hash the enum. |
| P-R6 | L | `render/layers.rs:291-296` | The people, pollution and crime grids rebuild every 1-2 s even while paused. | Also gate on the tick. |
| P-R7 | L | `render.rs:1996` | The walk-detail key uses `buildings.len()`, which is both stale and a scan. | A buildings revision. |
| P-O1 | M | `tools.rs:678-684,1152` | `EffectivePlan::sync` clones the whole plan every frame of an Edit drag, because `amend` bumps the version. | Patch the dragged gesture in place. |
| P-O2 | L | `stub_marks.rs:31,60` | `unjoined_stubs()` runs twice every frame, unconditionally. | Compute once into a resource keyed on versions. **REFACTORED 170677e (the labels reuse the rings' list while the Sim change tick holds)** |
| P-O3 | L | `ferry_select.rs:268,337` | `net()` (signature plus lock) runs three times a frame; `visible()` and `offset_polyline` run per line per frame. | Cache once per frame and with the network. **REFACTORED 170677e (`WaterFrame`, reused while the Sim change tick holds; `visible()` per frame left)** |
| P-O4 | L | `tools.rs:1236`, `ui/hover.rs:17` | Linear point-in-polygon over every lot while hovering. | `parcels.at(p)`. |
| P-O5 | L | `tools.rs:1925,1941,2014,2021,2061` | Rail-preview O(B·F) scans; `plant_group_of`, `owned_with` and `jam_chain` every frame for a selection. | Cache by (selection, tick). |
| P-O6 | L | `build_highlight.rs:696`, `render/plot_hover.rs:342` | `materials.get_mut` every frame re-prepares a material. | One pulse material driven by a time uniform. |
| P-O7 | L | `render/junction_view.rs:69-79` | `ResMut<ToolState>` is mutated every frame, so `ToolState` is always "changed". | Write only on change. |
| P-UI1 | M | `ui/skyline.rs:43,50,76` | The building inspector sorts the whole skyline every frame, twice for a tower. | Cache the ranking per (generation, tick). |
| P-UI2 | M | `ui/inspector.rs:546,562,748,753,1174` | The building inspector scans all citizens, cars or buildings every frame. | Occupancy indexes, or cache by (id, tick). |
| P-UI3 | M | `ui/people.rs:91,146`; `ui/detail.rs:553-617,672` | Incomes are sorted twice per frame; the land page does a full lot scan per land use; `grid_day_ahead` runs every frame. | Compute in one pass and cache by tick or hour. |
| P-UI4 | M | `ui/committee.rs:225,242,365,500,536` | The meeting, minutes, the whole `Committee` and the settings are cloned every frame. | Borrow and collect the actions. |
| P-UI5 | L | `ui/emergencies.rs:136-145`, `ui/directory.rs:154-232`, `ui/build.rs:704`, `ui/portrait.rs:121` | Collecting before `take(200)`; 6-8 KPI passes per frame; the build catalogue rebuilt each frame; eager tooltip `format!`s. | `take` first; cache the KPIs in the directory snapshot; `OnceLock` the catalogue; `on_hover_ui`. |

**System count and change detection:**
- Render alone registers about 90 systems, all in `Update` and most without run conditions.
- Change detection (`Changed<>`, `resource_changed`) is barely used; the code relies on hand-hashed signatures.
- **The single biggest structural win:** sim-side version counters (lots, buildings, parcels, vehicles and parking, plants), plus a `RebuildGate` that checks them before hashing anything.

---

## 5. Tests

**What exists:**

| Area | Tests |
|---|---|
| nd_app | `directory` 9, `render/look` 9, `ui/saves` 8, `ui/build` 5, `render/street_look` 4, `build_highlight` 3, `ui/history` 3, `ui/theme` 3, `remote/guard` 3 |
| nd_app | 1-2 each: `crash`, `map_layers`, `render/{districts,haze,plot_hover,shore,solar,swap,thumbs,turbines}`, `tools` (picking), `ui/city_data`, `ui/topbar`, `remote/property` |
| nd_minds | `committee/tests.rs` 16 (meeting flow, determinism, connectors against a fake HTTP server, key redaction), `lib.rs` 5 |

There is no `tests/` directory in either crate.

**Gaps, by value:**
1. **Tool behaviour:** draw, drag, undo restores the shape (B1). Undo during a stroke (B20).
2. **Remote dispatcher:**
   - A headless `App` with `MinimalPlugins` and the `Ctx` resources that runs every command's read form, checking the response shape and that nothing panics.
   - Help covers every dispatch name and alias.
   - `opt_f32` rejects non-finite values; `key_from` rejects bad ids.
   - HTTP 413, 415 and 504-withdraw behaviour.
3. **Selection:**
   - `thing_pose` for every variant (a table test).
   - The priority order in `pick_any`.
   - `road_select::pick`, upper level and underground.
   - `keep_on_rebuild`.
   - The reset on a new generation.
4. **Render on city reload:** a headless app that loads city A, then city B with reused ids, and asserts that no entity keyed by an old id survives (B4, B18). This would catch the next `sync_ships`.
5. **Committee:**
   - `Driver::run` keeps voice deliveries (#9).
   - A multi-day `step` runs `committee_daily` per day.
   - Remote `settings` never echoes the key, including after a 401.
6. **Pure UI helpers**, once extracted to `fmt.rs`: `money`, `compact`/`nice` at band edges, `minutes`, `plural`, `capitalise` (empty and multibyte input), `elide(n=0)`, `age_since`, and a `signed_bar_len` clamp (B7). Also a bubble-age function (B2).
7. **Build:** `cargo check --features heap-count` (#2).
