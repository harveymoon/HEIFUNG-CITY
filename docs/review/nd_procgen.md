# Code review: `crates/nd_procgen` (October 2026, main at a0a69df)

**Scope:** all of `crates/nd_procgen/src`, its `tests/` and `examples/`. Read-only.

**How it was done:**
- Five parallel slice reviews:
  - streets, walk, crossing, toll, preview, overlay, lights
  - `buildings/` core: mod, parts, kit, adjacency, district, brand, dirty
  - the style files supertall through waterworks
  - power, construction, industrial, trades, shore, docks, rural, dam
  - terrain, citygen, climate, social, society, vehicles, people, palette, textures, surface, metro, vacant, tests and examples
- `mesh.rs` and the cross-cutting greps were read by the consolidating reviewer.
- The top bugs were re-read by hand against the code: power.rs:135, rural.rs:485, entertainment.rs:167, the terrain and footbridge `-yaw`, walk.rs RNG, dirty.rs stains, and the supertall aviation lights.

**Build:** `cargo check -p nd_procgen --all-targets` is clean apart from one warning: the unused `hi` at `buildings/dirty.rs:441`. No tests were run.

**Severity:**
- **H**: a crash, or a visibly wrong result in normal play.
- **M**: a visible glitch, a real risk, or a cost that matters.
- **L**: hygiene.

All paths are relative to `crates/nd_procgen/src/` unless stated.

---

## Top 20 actions

### (a) Bug fixes

| # | Sev | Where | What | Fix |
|---|---|---|---|---|
| 1 | **H** | `buildings/power.rs:135` | `(plant_area*1.3/cw).clamp(26.0, d)` panics whenever a "roomy" lot is under 26 m deep. A 150×24 m lot at scale 1.0 has area 3600 > 1400, so cd = 36.1 and the call is `clamp(26, 24)`. It is reached from the mesh (`coal`, :359) and from the renderer's smoke (`coal_vents`, used by `nd_app/src/render/haze.rs:199`), so the game crashes. | `.max(26.0).min(d)`. Harden :425 (`p.x-h.x+1 .. p.x+h.x-1` inverts on halls under 2 m wide) and :386 the same way. **FIXED e338651** (sweep 56bcd7b) |
| 2 | **H** | `buildings/rural.rs:485` | `rim_pt.x.clamp(area.u0+3, area.u1-3)` panics when the footprint is under 6 m wide. The strip-mine business forces this style whatever the lot (`buildings/mod.rs:~1393`). | Guard on `area.w() > 6`, or use a `clamp_safe`. **FIXED e338651** |
| 3 | **H** | `buildings/entertainment.rs:167` | `hall_h = (...).clamp(lobby_h + 4.0, 20.0)` with `lobby_h = 2·fh`. `BuildingRules::validate` (`mod.rs:660`) allows `floor_height` up to 12, and the rules are a hot-reloaded RON file, so `floor_height > 8` crashes cinemas. This is the same crash class as the earlier renderer crash. | `.max(lobby_h+4.0).min(20.0f32.max(lobby_h+4.0))`. See also action 13. **FIXED e338651** (and BuildingRules::validate caps window_size, 56bcd7b) |
| 4 | **H** | `terrain.rs:686-692, 749-750, 878-898, 922-934, 975-977, 1058, 1073, 1120`; `footbridge.rs:69, 88` | Every `boxed(..., -yaw)` with `yaw = dir.y.atan2(dir.x)` is mirrored. `MeshData::boxed` (`mesh.rs:233-237`) maps local +x to world `(cos yaw, sin yaw)`, so `-yaw` gives `(cos, -sin)`. Boxes on any non-axis-aligned way are turned by 2a. On a 45° viaduct the pillar crosshead runs along the deck, and portal copings, abutments, piers, the pontoon and footbridge steps and columns are misaligned. The road hologram reuses these (`preview.rs:131,146`). | Pass `yaw`. Add a 45° viaduct, tunnel, bridge and footbridge fixture that checks each box's long axis against the way. **FIXED e338651** (also the trades bunting flags); 45-degree test e3288fb |
| 5 | M | `walk.rs:118-238` | One RNG stream per segment is consumed only for items that pass `near()`. When anything crosses the 90 m radius, every later item on that segment changes type and position, so carts and signs reshuffle on every 25 m redraw. This contradicts the module doc. | Draw a per-slot seed unconditionally (`let s: u64 = rng.random()`), build the item from `rng_for(s, ..)`, and cull afterwards. Test that the vertices in the overlap of two walker positions are identical. **FIXED 8501486** |
| 6 | M | `buildings/construction.rs:106-147, 259, 274, 519, 569-576, 771` | Site clutter shares one stream with `structure()`, whose draw count depends on the floors done, and with the crane and scaffold draws. Stacks, spoil, the mixer truck and the crew jump at every progress rebuild. `demolition_site` has the same problem. | Give each block its own `rng_for(c.seed, SALT)`, as the crane billboard already does (:578). **FIXED 8501486** |
| 7 | M | `buildings/supertall.rs:427, 563, 615` (aviation); `:556/559, 603/606`; `commercial.rs:315`; `towers.rs:590-605` | Aviation lights are placed on the base plan at every height, but the deco, twist and needle shafts taper or rotate: up to about 10 m in the air on a 420 m needle. Window lights and lit bands on twisting or tapering sections sit on `prev` while the glass lofts to `next`: up to 1 m proud or buried. The glass-castle seams ignore the ±0.15 rad rotation. | Pass a `height -> plan` closure to `aviation_up`. Emit per-floor lights on `lerp(prev, next, t)` as `residential.rs:719-741` already does (factor it out: action 14). **FIXED e2fbd56** (aviation_up takes the plan at each height; lofted lights per row; twist tower and glass castle seams) |
| 8 | M | `buildings/adjacency.rs:319, 387` | The StyleBook key (`key_of`, `role`) leaves out brand HQ and annex, which force a style (`mod.rs:1297-1299`). The book's label (inspector, remote API) can then disagree with the mesh. `role + 8` for a rooftop pad can also collide with other role ranges. | Make the key a struct with separate fields for role, rooftop, look, brand role and annex. **FIXED 03e25ab** |
| 9 | M | see list | Z-fighting cluster. Each item is 0 to 3 mm apart, or coplanar with mismatched UVs: (i) floor-band loft caps coplanar with the section's top cap: `supertall.rs:290, 555`, `commercial.rs:313`, `commercial.rs:329`; (ii) junction corner fill over the pavement ribbon, both at `KERB_H-0.01` with different UV bases: `street.rs:2541-2565`; (iii) soot stain k=5 at exactly 0.08, the feedlot pen height: `buildings/dirty.rs:159` vs `:354`; (iv) graffiti at 0.02/0.03/0.035, the same as window frames (0.03) and grime (0.02): `buildings/parts.rs:1690-1735`; (v) toll plaza lines at 0.085 = the road's `MARK_Y`: `toll.rs:102,123`; (vi) a lane-count joint with a bend leaves two untrimmed carriageways at `ASPHALT_Y`: `street.rs:259, 981`; (vii) deco pier glass 3 mm from the dynamic-light dark-glass copy: `supertall.rs:400`; (viii) balcony decal on its slab face: `towers.rs:229-230`; (ix) mechanic oil stains vs bay floor at 0.05: `trades.rs:356,394`. | Introduce named depth layers (action 15) and move each item off them. Use an uncapped `band` for floor bands (action 14). **FIXED** except (vi): (i) e2fbd56; (ii) fce8ae7; (iii) (iv) (v) (vii) (viii) (ix) 4a20490. (vi) NOT fixed: the lane-count joint needs the run/joint trimming reworked (phase 2b, with #17) |
| 10 | M | `metro.rs:56-73` | Each tunnel leg is a `strip_box` at its own midpoint depth, so tunnels step and gap on slopes and gap or overlap at bends. `:95` `p + n * 0.0` is dead. | `path_box` (it mitres) with per-vertex heights, or a `slab` per leg. **FIXED 2bf878e** |
| 11 | M | `buildings/mod.rs:745`; `services.rs:496`; `waterworks.rs:231`; `terrain.rs:40` | Robustness. `pick_style_near` checks for NaN and ≤0 but not infinity (`random_range(0..inf)` panics). `x[i % len.max(1)]` panics on an empty palette list. `terrain_height` divides by `half_size - basin_radius`. | `!(total > 0.0 && total.is_finite())`; use `palette::at` or `pick`; `.max(1e-3)`. **FIXED 56bcd7b** |

### (b) Behaviour-preserving refactors

| # | Sev | Where | What |
|---|---|---|---|
| 12 | M | `buildings/mod.rs:1490`, `dirty.rs:50`, `parking.rs:58`, `power.rs:884`, test `ctx_on` `mod.rs:2390` | One `Frame::local_poly(lot)` (cleaned at 0.3, rectangle fallback, counter-clockwise). Five copies of the lot→local outline feed things drawn outside the mesh (smoke vents, solar trackers, parking bays) that must match it exactly. `ctx_on` skips `clean_poly`, so tests use a different polygon. Also make the refinery vent share `refinery_stack(poly)` (`dirty.rs:125` vs `industrial.rs:844`). |
| 13 | M | whole crate | **A property sweep test** (`tests/style_sweep.rs`). Cover every `Style::ALL` × floors {1, 2, 6, 12, 40, 250} × tier × condition {0, 0.5, 1} × dynamic lights on/off × district look × brand HQ. Use synthetic lots: 6×30, 9×9, 150×24, 24×150, triangle, sliver, L. Also: plant scales {0.3, 1, 2.4, 3} for coal, nuclear and geothermal; rules at the validate limits (`floor_height` 2.2 and 12; `window_size > fh`); an empty-list palette; and construction and demolition at stages 0..1. Assert: no panic (collect failures with `catch_unwind`), finite attributes, valid indices, equal attribute lengths, no zero-area triangles, normals agree with winding, output byte-identical on a second run, and inside the lot. Each of bugs 1, 2, 3, 11 and the B2 graffiti fights would have been caught. **DONE 56bcd7b** (`buildings/sweep_tests.rs`; in-crate so it can force styles and extras) |
| 14 | M | `mesh.rs` | **Grow the MeshData toolkit**, then port the call sites (listed in §2):<ul><li>`band(bottom, top)`: sides only.</li><li>`lathe(center, &[(r, y)], sides)` on `strip()`.</li><li>`decal2`: double-sided.</li><li>`append_transformed(src, at, yaw)`.</li><li>`frustum` cap flags.</li><li>`ground_fan` moved here from `street_junction.rs:202`.</li><li>`stations(path, a, b, step)`: a monotonic polyline cursor.</li></ul>Per-floor `loft_lights` belongs in `kit.rs`. **PARTLY e2fbd56**: `MeshData::band` / `parts::band` added and used for floor bands and hidden-cap rings; the rest is phase 2b |
| 15 | M | `buildings/parts.rs:86-96` plus copies | **One `color.rs`** for `rgba`, `mul`, `mix` and `at`, replacing `vehicles.rs:219-229`, `people.rs:135`, `social.rs:463` `lerp3`, `society.rs:278` `mix_dim`, `kit.rs:203` `pick_led` and `toll.rs:61-62`. **One `depth.rs`** with the named decal and ground layers (frame 0.03, pane 0.05, light-glass −0.013, graffiti band, ground 0.02/0.045/0.08 …), and the street constants made `pub(crate)` (`walk.rs:23-24` and `crossing.rs:24` copy them, and `PAVE_Y` is 5 mm off). |
| 16 | L | `buildings/trades.rs:19-200` | Move the de facto toolkit (`R`, `fit`, `block`, `pane`, `pane_row`, `flagpole`, `table`, `fish_sign`) and `rural::{rail_fence, heap, cow, haul_truck}` into `kit.rs`. Add `parts::aviation_light` (13 copies), `parts::toward` (8), `Ctx::all_inside` (5), and `kit::crown_frame` (5). Delete `supertall.rs:58 point_in` and `estates.rs:498 point_in_poly`. |

### (c) Bigger restructures

| # | Sev | Where | What |
|---|---|---|---|
| 17 | M | `street.rs:796-1653` | `generate_street` is a single 858-line function. Split it into a `StreetCtx` (runs, profiles, lifts, corners, tapers, cuts, pad_mouths, gores) plus `draw_run`, `draw_aprons`, `draw_nodes`, `draw_corners`, `draw_midblock` and `draw_signals`. Make `street/` a directory: kerb, signals, rail_path, arrows, tunnel, tests. |
| 18 | M | `buildings/mod.rs` (2,430 lines) | Split it into `style.rs`, `rules.rs`, `frame.rs` and `choose.rs`, plus `tests.rs` (about 820 lines). Generate the enum, `ALL`, `label`, the dispatch and the trait tables from one `styles!{}` macro: five parallel 118-entry tables today. Remove the `_ =>` arm in `adjacency::traits` (`adjacency.rs:176`): it silently makes about 25 styles neutral. Split `power.rs` (packer, coal, nuclear, solar, wind, battery, field), `terrain.rs` (heightfield, water, works, docks, retaining) and `parts.rs` (polygons, facades, roofs, site, props, graffiti). |
| 19 | **H** (perf) | `nd_app/src/render.rs:3100-3112` → `terrain.rs:324, 434`, `waterworks` | Every road edit rebuilds the whole landscape: about 405k triangles, with a `ground_height` per vertex. It also rebuilds the water sheet, which does not depend on roads (about 774k `water_distance` calls, one `Vec` per cell at `terrain.rs:465`), and every port and boat. Fixes: give the water its own landscape-only signature; cache corners into an (n+1)² array; chunk the terrain into 128 m tiles and rebuild only those touched (this also enables frustum culling). `sync_scatter` has the same problem: every cell already has its own RNG, so make it incremental. |
| 20 | M (perf) | `buildings/parts.rs:54-84`; supertall; hidden caps | `window_light` allocates a 6-Vec temporary mesh per window, and doubles every lit window with a glass copy. A Bundled megatall carries 65-75k triangles of lights, with about 25% of the wing lights inside the core (`supertall.rs:470`). Stacked capped cylinders (cooling towers, domes, twist sections) spend about 45% of their triangles on invisible caps. Copy Glow→Glass in place; skip faces whose midpoint is inside the core; use `band` and `lathe`; then add a far LOD: rows halved above 100 m when not dynamic, and no grit, litter or furniture beyond about 300 m. |

---

## 1. Correctness

### 1.1 Panics (`f32::clamp` with min > max, empty ranges, empty lists)

`f32::clamp` asserts `min <= max` in release builds too. I checked about 300 `.clamp(` calls. Almost all have literal bounds, or bounds that the surrounding code keeps ordered. The exceptions:

| Sev | Where | Problem | Fix |
|---|---|---|---|
| H | `buildings/power.rs:135` | Top-20 #1. | `.max(26).min(d)` **FIXED e338651** |
| M | `buildings/power.rs:425` | `clamp(p.x-h.x+1, p.x+h.x-1)` on turbine halls under 2 m wide; the fallback size is 0.7× that. | `let m = (h.x-1).max(0)` **FIXED e338651** (`clamp_safe`) |
| L | `buildings/power.rs:386` | The same pattern on the boiler half-width. Safe today, because boilers are at least 2.7 m. | as above **FIXED e338651** (`clamp_safe`) |
| H | `buildings/rural.rs:485` | Top-20 #2. | guard **FIXED e338651** |
| H | `buildings/entertainment.rs:167` | Top-20 #3. | see table **FIXED e338651** |
| M | `buildings/mod.rs:745` | Infinite weight total. | `is_finite` **FIXED 56bcd7b** |
| L | `buildings/parts.rs:752` (`grime`) | An empty `random_range` when `face.len` is 0. | return if `len < 0.5` **FIXED 56bcd7b** |
| L | `buildings/services.rs:496`, `buildings/waterworks.rs:231` | `list[i % len.max(1)]` on an empty list. | `palette::at` **FIXED 56bcd7b** (also trades bunting, toll, industrial, sanitation, estates, crossing `[0]` indexing) |

Recommendation: add `fn clamp_safe(x, lo, hi)` (returns the midpoint when `lo > hi`) to `parts.rs`. Add a CI grep that flags `.clamp(` whose first argument is not a literal, for manual review. **DONE e338651**: `mesh::clamp_safe`; every `.clamp(` with computed bounds was checked and the unsafe ones (also construction crane mast, trades bay posts, tunnel portal lid) made safe.

### 1.2 NaN and degenerate geometry

The core primitives are now defensive:
- `quad` drops zero-area quads and splits along the inside diagonal; REVIEW2 L10 is fixed (`mesh.rs:189-221`).
- `tri_facing`, `strip_box`, `tube`, `beam` and `slab` return early; REVIEW2 L12 is fixed (`mesh.rs:546`).
- `uv_basis` has fallbacks.
- No unguarded `.normalize()` remains on input that can be zero (22 sites checked).

Remaining:
- **L `mesh.rs:300-311` `ground_strip`, `:269` `ground_polygon`:** zero-length legs emit zero-area triangles. There is no NaN, because the normal is fixed. Skip legs under 1e-4 m, and return when |area| < 1e-6. **FIXED 56bcd7b** (each ground triangle wound up on its own, slivers dropped)
- **L `nd_sim` `point_along`:** returns `d = 0` on a zero-length first leg. Its street callers then build `right = 0`, putting furniture on the centre line (`street.rs:473, 1205, 1712`). Skip zero legs when picking `d`.
- **L `mesh.rs:365-380` `cap`:** the `length_squared < 1e-12` test passes NaN rings (a NaN compare is false), so NaN normals are pushed. Use `!(len2 > 1e-12)`. **FIXED 56bcd7b** (also per-triangle winding and collinear slivers dropped)
- **L `mesh.rs:405-435` `frustum`:** uses `n = min(len)` for the sides but caps the full `top` and `bottom` rings. Truncate both rings to `n`. **FIXED e2fbd56**
- **L `mesh.rs:141` `MeshData::append`:** extends `tangents` blindly; a source without tangents shifts every later tangent. Add `debug_assert!` that all attribute lengths are equal, in `append` and at the end of each generator.
- **L `buildings/parts.rs:142-150` `offset_poly`:** on C- and L-shaped lots `inradius` is measured from an area centroid that can lie outside the lot, and the `scaled()` fallback scales toward that outside point. Use `interior_anchor` + `distance_to_boundary`.
- **L `buildings/industrial.rs:24-29` `gable_roof`:** `loft` silently draws nothing (`parts.rs:408`) when `offset_poly`'s cleanup drops a vertex, leaving an open-topped shed. Build the top from the offset ring itself.
- **L `buildings/parts.rs:1118-1128` `bollards`:** `reach_out - 0.4` goes negative with no forecourt, putting the bollards inside the wall. **`:1140` `tank_horizontal`:** a negative saddle height when y < r. **`commercial.rs:391`:** zero-height props. **`construction.rs:928-955` `clip_above`:** blended normals are not renormalised. (`clip_above` normals renormalised and grazing slivers dropped: **FIXED 56bcd7b**; the rest not done)
- **L `terrain.rs:83-118` `build_far_ground`:** the inner edge differences against 0 (unfilled NaN read as 0). Widen the margin to 2.5 cells.

### 1.3 Placement, orientation and normal errors

- **H** The `-yaw` mirror in terrain and footbridge (Top-20 #4). It was verified against `mesh.rs:233-237` and against the `+atan2` callers in `buildings/parts.rs:1817` and `vacant.rs:161,194,209,298`. **FIXED e338651**
- **M** The supertall aviation lights and lofted lights (Top-20 #7). **FIXED e2fbd56**
- **M `residential.rs:530`:** on monumental brutalist blocks, `band_windows` starts at `y0 + fh` while the mass starts at `1.6·fh`, so the first band hangs under it over the pilotis. Start at `y0 + ground_h`. **FIXED d27be2f**
- **M `entertainment.rs:287-288`:** the arena light bands are at constant radii while the shell lofts r→1.12r→1.05r: one floats 1-2 m out, the other is buried. **FIXED d27be2f**
- **L `residential.rs:656-661`:** courtyard windows run past a shortened wing. **`civic.rs:45`:** precinct barriers run past the face end on fronts over about 50 m. **`towers.rs:39-40`:** wall surface and colour are picked independently (a panel texture can get a plaster tint).
- **L `brand.rs:31-51, 239-276`:** banners and logos sit on the axis-aligned box of the upper mass, so they float on twisted or rotated towers.
- **L `crossing.rs:107-110, 139-142`:** the crossbuck shows solid red from behind; the white board's back face sits in front of the red one's.
- **L `street.rs:1043, 1067, 1098`:** in the motorway loop, a local `run_z: Vec` shadows the `run_z` closure and ignores `inner` samples. The barrier `blocked()` test and lamp placement then use the wrong height on hills. **`:832` vs `:947`:** `grit` re-derives "worn" from a different midpoint than `run_worn`; use `run_worn[ri]`. **FIXED fce8ae7** (the motorway loop; it actually read the first node's height everywhere, the lists differ in length). The `grit` worn mismatch is not done.
- **L `terrain.rs:303-319` `raise_geometry`:** raises parts, pads and cranes but not `lamps`. Latent: only street lamps are consumed today.
- **L `social.rs:1014-1016`:** garden beds cluster in one corner on large plots; centre the grid.

### 1.4 Z-fighting

See Top-20 #9 for the nine confirmed cases. Also:
- Boats: glass bands and strakes 1.5-2.5 cm proud of the hull (`terrain.rs:1394, 1452-1456, 1472`).
- Mural lettering shadow and letter 6 mm apart (`social.rs:786-787`).
- Dam foam patches 2 mm apart (`dam.rs:177`).
- `flat_bar` back-to-back coplanar quads (`parts.rs:1352`), which matter only if a double-sided material is ever used.

The root cause in every case is the same: depths are bare literals. Many land on the same 1 cm grid, which `LIGHT_GLASS_BEHIND` (`parts.rs:41`) already documents as a trap. Fix it structurally with a `depth` module (Top-20 #15) and a ratcheting coplanar test (§5).

### 1.5 Determinism

- **Same seed, same mesh holds for buildings, citygen and streets.**
  - Every building draws from `rng_for(seed, salt)`.
  - Every `HashMap`/`HashSet` is used only for lookups or `contains`: `street.rs:263, 730, 834, 860-861, 1980, 2062, 2165, 2387`; `walk.rs:46`; `adjacency.rs:312, 400`; `citygen/graph.rs:681`; `citygen/mod.rs:430, 588`; `people.rs:583`.
  - Sorts use `total_cmp` with tiebreaks.
  - `streets_are_finite_low_and_deterministic` and citygen's `check_deterministic` assert this.
  - There is no `thread_rng` or wall clock outside one ignored test (`street.rs:3195`).
  - REVIEW2 L19 (FigureCache order) is fixed: it is now a BTreeMap (`people.rs:430-436`).
- **Stability across rebuilds is the weak spot.** Different inputs should not reshuffle unrelated output, and today they do:
  - walk detail (Top-20 #5) **FIXED 8501486**
  - construction stages (Top-20 #6) **FIXED 8501486**
  - `terrain.rs:150/215`: the scatter cap is a running count, so clearing one cell can make a later cell appear. Decide survival per cell by hash instead. **FIXED 551fba8**
- **Fragile salts:**
  - `industrial.rs:1024` uses `91+i`, overlapping the crane's 91-93.
  - `supertall.rs:252` borrows `nd_sim::fishing::hash`, and `shore.rs:73` re-implements it.
  - `citygen/mod.rs:574` and `climate.rs:252` have two different `hash01`s. The climate one is `fract(sin(x)*43758)` on f32: it depends on libm, and is badly quantised at city coordinates × 78.
  - Fix: one `crate::hash` module with `hash2(u64, u64)`, `hash01_pos(seed, Vec2)` and named salt constants.

---

## 2. Duplication and a shared toolkit

### 2.1 Proposed shared toolkit

| Module | Contents | Replaces |
|---|---|---|
| `mesh.rs` (extend) | `band(bottom, top)`: no caps | the closed frustums and extrudes used as rings or bands: `towers.rs:239, 315-317, 340-344, 605`; `supertall.rs:290, 360, 432, 555, 619, 748`; `residential.rs:749`; `entertainment.rs:287-288`; `commercial.rs:110, 313, 330` |
| | `lathe(center, profile, sides)` on `strip()` | coal cooling tower `power.rs:342`, nuclear towers `:1270`, dome `:1243`, smokestack `:267`, `parts::chimney :1151`, `tank_vertical :1131`, `rural::silo :84`, `feed_silo :96`, dam intakes `dam.rs:186`, museum dome `estates.rs:798-805` |
| | `decal2(...)`: double-sided | `crossing.rs:107`, `walk.rs:231-235, 263-264, 270-272, 279-281, 338-341` |
| | `append_transformed(src, at, yaw)` | `terrain.rs:1347 stamp_mesh`, `vehicles.rs:1075-1093` |
| | `blob(c, r, squash, rot, n, wobble)` | potholes `street.rs:1725-1731`, puddles `walk.rs:360-367`, stains `dirty.rs:152` |
| | `ground_fan` (move) | `street_junction.rs:202-237` |
| | `stations(path, a, b, step)` iterator | about 14 `while s < len { point_along(..) }` loops: `street.rs:187, 472, 788, 1093, 1188, 1220, 1231, 1249, 1279, 1719, 1738, 1757, 1901, 1908`. It is also faster (§4). |
| `color.rs` (new) | `rgba`, `mul`, `mix`, `at` | `parts.rs:86-96`, `vehicles.rs:219-229`, `people.rs:135`, `social.rs:463`, `society.rs:278`, `kit.rs:203`, `toll.rs:61-62`; inline mixes at `street.rs:952`, `preview.rs:194`, `overlay.rs:132`, `terrain.rs:122, 365` |
| `depth.rs` (new) | named decal and ground layers, and the street heights (`KERB_H`, `ASPHALT_Y`, `MARK_Y`, `PAVE_TOP`) | literals everywhere; `walk.rs:23-24`, `crossing.rs:24` |
| `hash.rs` (new) | `hash2`, `hash01_pos`, salts | `shore.rs:73`, `citygen/mod.rs:574`, `climate.rs:252`, `parts.rs:65`, `supertall.rs:252` |
| `buildings/kit.rs` (extend) | `R`, `fit`, `block`, `pane`, `pane_row`, `flagpole`, `table`, `fish_sign` from `trades.rs:19-200`; `rail_fence`, `heap`, `cow`, `haul_truck` from `rural.rs`; `loft_lights` from `residential.rs:719-741`; `crown_frame`; `site_perimeter` | see below |
| `buildings/parts.rs` (extend) | `aviation_light`, `toward(p, centre, d)`, `Ctx::all_inside`, `lamps_along(face, n, d, h)`, `patchwork` (wall surface and colour), `glazing(&GlazeSpec)`, `clamp_safe`, `Frame::local_poly` | below |

### 2.2 Duplication clusters (every site)

- **Aviation light box (13 copies):** `supertall.rs:338-345, 364-365`, `commercial.rs:277, 323`, `residential.rs:752, 754`, `civic.rs:205`, `services.rs:340`, `air.rs:154, 293`, `rail.rs:190`, `subway.rs:368`, `waterworks.rs:251`.
- **Window and glazing loops (6 implementations), each with its own depth numbers:** `parts::windows :516`, `band_windows :548`, `curtain_lights :572`, `storefront :595`, `kit::dense_windows :86`, `trades::pane/pane_row :147,160`. Unify them behind one `glazing(..., &GlazeSpec)` that owns the frame, pane and light depths and the `window_light` call.
- **Per-floor lights on a lofted shaft (5 implementations; only one is correct):** `supertall` twist and needle, `commercial::twist_tower`, the `towers` glass_castle seams, `residential` crystal (correct).
- **Tower base size `(t.height/K).max(M).min(room·x)`:** `supertall.rs:94-99` (`foot`), plus copies at `:384, 465, 530, 584, 671-676`.
- **Inset a corner toward the centre (8 copies):** `commercial.rs:389`, `civic.rs:52, 122`, `entertainment.rs:310`, `services.rs:580`, `residential.rs:252, 290`, `estates.rs:277`.
- **All corners inside the lot (5 copies):** `estates.rs:82`, `dense.rs:~299`, `residential.rs:331`, `towers.rs:542`, `entertainment.rs:22`.
- **Crown frame (posts plus `glow_outline`):** `towers.rs:131-137, 246-253, 512-515`, `supertall.rs:564-570`, the `dense.rs` neon_tower crown.
- **Lamps along a face:** `towers.rs:288-291, 525-527`, `commercial.rs:338-340`, `supertall.rs:178-185`.
- **Wall-finish pickers:** `dense.rs:15 patchwork`, `commercial.rs:28-32`, `towers.rs:39-40` (the buggy one).
- **Gable roofs (4):** `industrial.rs:24`, `rural.rs:70, 77`, `towers.rs:17`.
- **Fences (4):** `parts::fence :1061`, `rural::rail_fence :47`, `vacant::fence_run :155` (and `hedge_run :187` vs `parts::hedge :1591`), `construction::hoarding :152`.
- **Site perimeter:** `construction.rs:43-68` is a verbatim copy of `:739-762`.
- **Café table:** `trades::table :254` = `shore::world_table :912`.
- **Boats:**
  - `shore::boat :927` is a box version of `terrain::boat_mesh :1438`.
  - `terrain::ship_mesh :1489-1495` re-implements `hull :1366` and hard-codes sizes that `boat_size :1239` already holds.
- **Floodlight masts:** `trades::mast_light :207` vs `parts::floodlight :1615`.
- **Geometry utilities:**
  - `construction.rs:969 convex_hull` = `nd_sim::estate::convex_hull`.
  - `construction.rs:992 polygon_inside` = `kit::poly_inside_poly`.
  - `supertall.rs:58` and `estates.rs:498` are wrappers of `nd_sim::zoning::point_in_polygon`.
  - `society.rs:58 ring` = `MeshData::ngon`.
  - `park.rs:555 circle`, `park.rs:566 rail` (= `beam`), `estates.rs:92 picket` (≈ `fence`).
- **Bridge and viaduct deck (slab, rails and parapets, with drifting constants):** `terrain.rs:579-588, 950-958, 1017-1024`. Viaduct columns `:966-977` and bridge piers `:1066-1074` re-do `pillar_column :681-693`. Consolidating them also fixes the yaw bug in one place.
- **Height-field builders:** `terrain.rs:72, 226, 324` repeat the colour mix, the finite-difference normals and the index loop. `mud` is defined twice (`:98`, `:333`). Water sheets `:434` / `:506`. Use one `heightfield_mesh(n, cell, origin, h, colour_fn)`.
- **Weighted pick (3):** `mod.rs:742`, `mod.rs:1439` (`pick_industry_style`, which also ignores `site_fit`), `district.rs:77`. Use one `weighted_pick`, which also fixes the infinity check.
- **Streets:**
  - Zebra stripes 4×: `street.rs:1371-1375, 1471-1475, 1578-1582, 1687-1693`.
  - Live street arms at a node 7×: `street.rs:539, 849, 1350, 2031, 2172`, `street_junction.rs:60, 103`. Use `RoadNetwork::live_streets_at` in nd_sim.
  - Run end segments recomputed: `:837-840, 964-965, 975, 1138`. Store them on `Run`.
  - Lift-since blocks: `:1326-1332, 1575-1590, 1602-1610, 1621-1650`.
  - Cracks as 12-triangle boxes (`street.rs:1747, 1750`) vs 2-triangle decals (`walk.rs:388-390`).
- **Ctx mirrors Extras field by field:** `mod.rs:1505-1530`, and again in `ctx_on :2390`. Hold `&Extras`, or use one `Ctx::new`.
- **Test helpers:**
  - mesh checks 3×: `check_mesh` in mod.rs, `economy_tests::valid`, inline in `dirty_tests`.
  - lot builders 4×.
  - street triangle helpers 5×: `street.rs:2902-2929, 2943-2950`, `street_junction.rs:488-501, 601-614, 715-724`.
  - Use `buildings/test_util.rs` and `street/test_util.rs`, or one crate-level `#[cfg(test)] mod testkit`.

### 2.3 Palette bypasses

These hard-coded colour tables ignore themes and climate, and a palette hot reload never changes them:
- `social.rs:31 PAINT`, `:368 NEON`, `:369 DEEP`.
- `vehicles.rs:48 PAINT`, `:60 NEON`. `draw_parked` also builds the model with a random paint and discards the `color` it is given.
- `metro.rs:13 LINE_COLOURS`.
- Street, walk, toll and footbridge literals: `street.rs:1586, 1764`, `walk.rs:219, 316, 324, 456, 460`, `footbridge.rs:26, 43`, `toll.rs:64, 182`, `crossing.rs:116`.
- Direct `pal.concrete[0]` indexing: `industrial.rs:743, 789, 1038`, `crossing.rs:124`.

Add named palette entries (`beacon`, `cone`, `lantern`, `hazard`, `barrier_red`, `line_colours`) and always go through `palette::at` / `pick`.

---

## 3. Structure

### 3.1 Oversized functions (measured)

| Lines | Function | Split into |
|---|---|---|
| 858 | `street.rs:796 generate_street` | Top-20 #17 |
| 316 | `buildings/residential.rs:79 stacks` | place_units, dress_unit, props, planks, utilities |
| 235 | `textures.rs:94 sample` | one fn per surface (acceptable as is) |
| 228 | `terrain.rs:985 waterworks` | bridges, docks, water_plants, vents |
| 213 | `buildings/park.rs:98 park` | layout, paths, planting, furniture |
| 213 | `buildings/mod.rs:1472 generate_styled_extras` | ctx build, demolition branch, dispatch |
| 207 | `walk.rs:42 walk_detail` | fronts, the per-side pavement loop, lifting |
| 179 | `buildings/construction.rs:722 demolition_site` | perimeter (shared), cut structure, rubble |
| 169 | `toll.rs:54 toll` | `plaza_apron`, booths, canopy |
| 168 | `buildings/subway.rs:265 central_station` | |
| 161 | `buildings/shore.rs:610 beach` | |
| 154 | `social.rs:185 facade_spot` | wall_planes, wall_grid, blocked_grid, spot_from_rect |

Also near the limit:
- about 150 lines: `trades.rs:345 mechanic`, `towers.rs:146 twin_slab`, `towers.rs:295 megastructure`, `estates.rs:754 museum`
- about 145 lines: `estates.rs:509 mansion`, `power.rs:356 coal`, `dam.rs:45 hydro_dam`
- about 140 lines: `trades.rs:931 golf_range`, `street.rs:2428 corner_between`

### 3.2 Files to split

- `street.rs` (3,315 lines, about 750 of them tests). Use `street/{mod, kerb (:2091-2672), signals (:1949-2089), rail_path (:1773-1930), arrows (:3245-3370), tunnel (:99-203), tests}.rs`. This replaces the `#[path]` include of `street_junction.rs`. `shelter` (`:1932-1947`) is used only by `buildings/transit.rs:105`; move it there.
- `buildings/mod.rs` (2,430): `style`, `rules`, `frame`, `choose`, `tests`, plus the `styles!{}` macro (Top-20 #18).
  - The `dirty_tests.rs:51-53` assertion `ALL.len() == 112 + 6` exists because the tables are parallel.
  - No test catches a variant missing from `ALL`.
- `buildings/parts.rs` (1,695): polygons, facades, roofs, site, props, graffiti and ground.
- `buildings/power.rs` (1,451): packer, coal, nuclear, solar, wind, battery, field.
- `terrain.rs` (1,540): heightfield, water, works (viaducts, portals, abutments), docks (boats, moorings, stamp), retaining.
- `buildings/trades.rs` (1,221): move its shared kit out (Top-20 #16).

### 3.3 Dead code (grepped across all crates, nd_app included)

- `terrain.rs:288-300` `lift_geometry`, `lift_mesh`: no callers, and their doc is stale. Delete.
- `buildings/mod.rs:949` `Frame::is_world` (still advertised at `:924`); `mod.rs:685` `TierStyle::wall_color`.
- `preview.rs:36` `road_preview`: called only by its own test; the app uses `road_preview_with`.
- Public but module-only: `street::travel_arrows :469`, `street::bore_profile :117`, `overlay::CAR_SPACE_M`, `crossing::PIVOT_Y`. Make them `pub(crate)`.
- Dead bindings:
  - `supertall.rs:54` `let _ = c;` (drop the parameter), `:410/433` `last`, `:429` `hd*0.24/0.78*0.78`
  - `estates.rs:395`, `services.rs:304`, `subway.rs:132` (`let _ = ...`)
  - `power.rs:629` (rng discarded)
  - `park.rs:98 _style_tier`, `shore.rs` `beach _rng`, `society.rs:286 _pal`
  - `shore.rs:93` `yaw + 0.0`, `metro.rs:95` `n * 0.0`, `terrain.rs:940` `let parapets = true;`, `street.rs:1079` `offset_path(&piece, 0.0)`
  - `dirty.rs:441` unused `hi` (the only compiler warning) **FIXED 56bcd7b**
- `citygen/mod.rs:466-472` `sample_way`: `ts` holds at most one value but is built as a Vec and sorted.
- `street.rs:230` `degree()` is a one-line wrapper; inline it.

### 3.4 Stale comments

- `terrain.rs:905-912`: the `viaduct_span` doc and `#[allow]` sit above `abutment`; `viaduct_span :939` has no doc.
- `walk.rs:23`: "matching the street generator's kerb". The kerb top is 0.15, not 0.155.
- `supertall.rs:741-743`: "plates drawn as rings, solid every fourth". `loft(poly, poly)` is a closed solid identical to `extrude`.
- `supertall.rs:500-504`: "vertical fins up the wing's nose" describes one base box.
- `buildings/mod.rs:205, 218, 269`: the society block is "kept last" / "at the end", which is no longer true.
- `buildings/parts.rs:1644-1646`: graffiti as "thin boxes"; it is decals, and `place` ignores its Vec3 z.
- `tests/probe_procgen_*.rs` now hold regression tests, not bug probes (headers updated). Rename them to `*_regressions.rs`.
- `examples/memory_probe.rs` measures `World` heap and save size, which is nd_sim's business. Move it to `nd_sim/examples`.

---

## 4. Performance

### 4.1 Rebuild costs (largest first)

| Sev | Where | Cost | Fix |
|---|---|---|---|
| H | landscape on every road edit | Top-20 #19 | separate water signature; chunked terrain |
| M | `terrain` `sync_scatter` (app) | regenerates all 3,500 pieces (about 40k Fbm cells with a ChaCha each) per road version | per-cell cache, or regenerate only cells near the changed segments |
| M | `preview.rs:60-63` | the road hologram clones the whole `RoadNetwork` and runs `grade_crossings()` on every cursor move (the holo cache key includes the cursor) | test the new legs against `segments_in_box` without cloning, or keep the clone between moves |
| M | `street.rs:2020-2022` `ped_signal_posts` | rebuilds runs and all junction corners on every lane-version change, repeating what `generate_street` just did | return `StreetOutput { geo, ped_posts, signal_groups }` |
| M | `street_junction.rs:342` `extra_stop_back`, `street.rs:3273/3280` `lane_arrows`, `street_junction.rs:310` `place_pole` | `mouths()` / `pad_outline` recomputed per lane or pole (O(arms²) each) | pass the `pad_mouths` map built at `street.rs:860` |
| M | `point_along` loops (`street.rs:1073-1077, 432-447, 375-384` + the about 14 furniture loops) | O(samples × legs); the barrier `blocked()` closure (`:1046-1069`) also recomputes `path_length` per taper per call and does a full-scan `profile_height` | the `stations()` cursor; precompute taper lengths; use `lifts[ri]` |
| M | `buildings/power.rs:63-114` layout packer | up to 3,600 candidates sorted per failed placement, about 10⁷ operations per big plant per rebuild | search outward from the wanted spot |
| M | `vehicles.rs:1053-1094` `draw_parked` | a full `build()` per parked car, then de-indexed (2-3× the vertices) | cache low-detail meshes per `variant_key`; `append_transformed` indexed |
| M | `social.rs:271-288` `facade_spot` | scans every building triangle for each of 6 planes, per mural | bucket the triangles by plane once |
| M | `nd_app/src/render/shore.rs:25-53` → `shore.rs:783 jetty()` | every jetty in the city is rebuilt on day/night flips and trawler moves, because the lamp colour and the moored boat are baked in | constant lamp colour dimmed by the renderer; boats as separate entities |
| L | `buildings/power.rs:1040` | `wind_rotor(phase)` is rebuilt per turbine (up to 48 per lot) | build once and stamp |
| L | `docks.rs:21` | `boat_on_land` rebuilds the boat per trailer | cache |
| L | `industrial.rs:1024` | `sort_by_key` builds a ChaCha per comparison | `sort_by_cached_key` or a hash |
| L | `parking.rs:157` | `offset_poly(poly, 7.0)` inside a per-bay filter | hoist it |
| L | `towers.rs:592-593` | `fit_inside` computed twice; `commercial.rs:309` runs it per floor | fit once |
| L | `walk.rs:46-66` | `fronts` rebuilt from every lot on each 25 m redraw | prefilter by distance |
| L | `district.rs` apply, `mod.rs:1500` | the whole Palette (15 Vecs), TierStyle and rules cloned per building with a look; Ctx clones ParkLayout and RailLink | cache per look; borrow |
| L | `mesh.rs:96` `triangulate` | O(n³) worst case (`idx.remove` plus a full blocked scan per candidate) | fine for lots; avoid it for big water or terrain polygons, or use a reflex-vertex list |

The per-frame cost is fine: `crane_slew` is cheap; `wind_turbines`, `solar_trackers` and `coal_vents` only re-run when their cache key changes; vehicle and people meshes are cached (`FigureCache`, 1,024 entries; its eviction is an O(n) scan per miss, which is minor).

### 4.2 Triangle counts and hidden geometry

- **Lights:** `window_light` (`parts.rs:54-84`) allocates 6 Vecs per window, and with dynamic lights adds a dark-glass copy, so every lit window costs 4 triangles. A Bundled megatall has about 65-75k triangles of lights. The only budget check is `< 400_000` per building (`supertall.rs:820`). About 25% of the wing curtain lights are on faces inside the core (`supertall.rs:448-470`). `lit_band` stacks on curtain lights for the same floors (`:301-307`).
- **Hidden caps:** `frustum` always caps both ends, and `Frame::bx` always draws its bottom. Stacked cylinders therefore carry internal caps: a coal cooling tower has about 264 hidden triangles against 288 visible; each nuclear dome about 180; the twist shaft up to 48 sections × 2 octagon caps; the museum dome has 5 capped 16-gons inside opaque glass. `band`, `lathe` and cap flags remove most of them.
- **Grit:** cheap-street cracks are `boxed` slivers of 12 triangles each, every 4-9 m on both sides (`street.rs:1747, 1750`); decals would save about 10 triangles each, tens of thousands in a poor district. Footbridge posts every 2.5 m are 12 triangles each (`footbridge.rs:47-51`).
- **Allocation:** every cylinder or frustum allocates several small Vecs (`ngon`, `tube` rings). Use `SmallVec` or stack arrays for n ≤ 16.

### 4.3 LOD

There is none, anywhere. The far ground ring (25 m cells) is the only coarse level. Buildings, streets, walk detail, toll booths, terrain (one 405k-triangle mesh, never culled) and boats are all single detail. The cheapest wins:
1. Chunk the terrain so it can be culled.
2. Add a "far" flag to `Ctx` that skips windows above 100 m (unless dynamic), grit, litter, furniture, graffiti and roof gear.
3. Put street furniture and grit in a separate surface layer the app can cull by distance.

---

## 5. Tests

### 5.1 What exists

- `every_style_builds_on_odd_parcels_without_nans` (`buildings/mod.rs:1742`): every style on the lots of one 200×140 zone, all land uses and tiers, at floors 6 and 2 and plant scale 1.0. It checks finite data and valid indices.
- `geometry_stays_near_its_lot`: extent, height and triangle budget at 2, 9 and 30 floors.
- Determinism: byte-identical output for 5 styles; streets; citygen presets; coal, nuclear and wind layouts.
- Coplanar: only HillsideSteps and window-vs-frame, at condition 0.9. The all-style `survey_coplanar` is `#[ignore]` and only prints.
- Streets: layer separation (All = Surface + Underground), coplanar asphalt at forks and ramp terminals, height-lift cost (REVIEW2 M9, M10 and M11 are all fixed and tested).
- `mesh.rs`: outward faces for the primitives, and the concave L footprint.

### 5.2 Gaps, in order of value

1. **The property sweep from Top-20 #13.** It is the single highest-value addition, and would have caught all three clamp panics and the cinema panic. Today Form::High (≥10 floors) is never NaN or index checked, nor are 1-floor buildings, condition 0 (graffiti, shutters, boarding), the rules at their validate limits, odd plant scales, or empty palette lists. **DONE 56bcd7b**
2. **Geometric soundness in the shared `check_mesh`:** no zero-area triangles; stored normal · winding normal > 0; equal attribute lengths. Use it for every generator, not just buildings: `build_landscape_terrain`, `build_far_ground`, `build_water`, `waterworks`, `retaining_walls`, every `boat_mesh` ShipKind, `trawler_mesh`, `society`, `social`, `vacant`, the footbridge, crossing furniture, `jetty()` (no tests at all) and `hydro_dam` on narrow rivers at scale 3.
3. **Orientation:** a 45° viaduct, tunnel, bridge and footbridge, asserting each box's long axis relative to the way. This catches Top-20 #4; the existing portal probe only uses an axis-aligned portal. **DONE e3288fb**
4. **Rebuild stability:**
   - walk detail: the vertices in the overlap of two walker positions are equal (the current test compares triangle counts for a 0.01 m move); **DONE 8501486**
   - construction clutter: unchanged across progress steps; **DONE 8501486**
   - scatter: clearing one cell changes only that cell. **DONE 551fba8**
5. **A coplanar ratchet:** turn `survey_coplanar` into a per-style baseline count that must not rise. Run it at condition 0 and 1, with dynamic lights. Add street cases: acute corners on Paving, a lane-count joint at a 20° bend, a toll plaza over a road.
6. **Lights on surfaces:** for the supertall designs, `twist_tower`, `glass_castle` and the arena, every Glow vertex lies within about 0.3 m of the shaft plan at its height. **DONE e2fbd56**
7. **StyleBook:** the key changes when brand, annex or rooftop changes (B3); `Style::ALL` is complete (generated by the macro). (key: **DONE 03e25ab**)
8. **Small units:**
   - `MeshData::append` with mismatched attributes;
   - `HeatGrid::splat` at the edges;
   - citygen `sample_way` and `landuse::allocate`, fast unit tests (today only the slow `check_city` integration tests cover them);
   - metro floor continuity on slopes; **DONE 2bf878e**
   - `surface.rs`, which has no tests.

---

## Appendix: status of earlier review items

| Item | Status |
|---|---|
| REVIEW2 M9: street height lift O(n·m) | Fixed: `RunProfile` (`street.rs:725-783`). A remnant remains at `street.rs:1067, 1098` (motorway loop, full-scan `profile_height`). **Remnant FIXED fce8ae7.** |
| M10: layer leaks | Fixed and tested. |
| M11: `walk_detail` and elevation | Fixed (`walk.rs:74, 243-245`) and tested. |
| L10: quad normals | Fixed (`mesh.rs:189-221`). |
| L11: ship deck double cap | Fixed (`terrain.rs:1497-1500`). |
| L12: zero-width slab NaN | Fixed (`mesh.rs:546-549`). |
| L14: crane `normalize_or_zero().max` | Fixed (`construction.rs:~519`). |
| L19: FigureCache HashMap order | Fixed (BTreeMap, `people.rs:430-436`). |

---

## Phase 2a (fix/review-procgen): what the sweep found beyond this review

The property sweep (`buildings/sweep_tests.rs`) turned up, and the branch fixed:
- `mesh::newell` worked in world coordinates, so a thin wire or tube hundreds of metres from the origin got a random ring winding and was drawn inside out (car-dealer bunting, string lights): 56bcd7b, test `mesh::wire_tests`.
- Brand pieces on concave or triangular lots (logos, banners, crown bands, the spire over an L's notch); wires and string lights across a lot's notch (`Ctx::spans_lot`); vertiport pads drawn at 4.6 m where only a 3 m spot was found; McMansion garage doors past the end of a fitted wing; villa and estate porch canopies off the lot; a 2 m helipad on a slab roof narrower than that: 56bcd7b.
- `BuildingRules::validate` let windows be taller than a floor and wider than their spacing (AC units then sank into the ground): 56bcd7b.
- Scaffold netting changed sides at every floor and its foot moved with the scaffold's top: 8501486.
