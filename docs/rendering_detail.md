# Close-up detail: how it is drawn

The one place that explains how HEIFUNG://CITY draws the small things you only
see up close: what each pass draws, how it fades, and where its shapes come
from. [r13-detail, 2026-11-21] Before this, several passes had their own fade
distances and their own copies of the same props; this page is the map after
they were unified. When you add a detail pass, pick a band from here and a
prop from the library. Don't add a private fade or a private bike.

## The rules

1. **One set of distance bands**: `nd_procgen::detail::DetailBand`
   (`crates/nd_procgen/src/detail.rs`).

   | band | drawn to | fades from | what goes in it |
   |---|---|---|---|
   | (base) | always | | the building mass, lit windows, signs, every glow, trees |
   | `Mid` | 700 m | 560 m | reads from a few blocks: awnings, cages, tanks, cores' steel, rack rails, laundry-pole frames on megablocks |
   | `Near` | 320 m | 256 m | the grain from the street: AC units, frames, pipes, railings, **every bench, bin and bike**, vacant-lot junk, garden-bed plants, park litter |
   | `Close` | 140 m | 112 m | what says who lives there or what the weather is: the household layer (washing, toys, bikes by the door, pots, grills, trampolines) |

   Every band fades over its last 20 % (`FADE_FROM` 0.8) through a Bevy
   `VisibilityRange`. The household layer is also not *placed* past
   `CLOSE_REACH` (150 m along the ground), or when the eye is more than
   `CLOSE_EYE` (220 m) up, so it costs nothing when zoomed out.
   `Lod::NEAR_RANGE` and `Lod::MID_RANGE` are the bands' ranges.

2. **One way to put geometry in a band.** A generator draws into a detail
   layer with `g.lod(Lod::Near, |g| ...)` (or `Lod::Mid`). In the app every
   path that turns a `Geometry` into entities gives a detail layer its
   band's `VisibilityRange` (`render/mesh_util.rs`: `detail_range`,
   `band_range`, `geometry_layers`):
   - building chunks (`render/chunk_jobs.rs spawn_layer`: detail cut into
     detail cells, each measured from its own bounds);
   - `spawn_geometry` (parks, vacant lots, cleanup sites, junction
     footbridges, waterworks, shore, streets' extras: everything
     world-placed);
   - `cell_jobs::spawn_meshes` (murals and community gardens, skid row);
   - the undercroft bays (`render/undercroft.rs`);
   - the household layer (`render/yards.rs`, `band_range(Close, false)`:
     measured to each prop's foot).

   Previews and the building viewer draw everything at every distance (their
   own spawns).

3. **One prop library**: `nd_procgen::props` (`crates/nd_procgen/src/props.rs`).
   The bicycle, the tree family, the bench, the bin, planters and pots, the
   garments and washing runs are built there once. Look differences are
   parameters, not copies:
   - `bicycle(g, Pose, Bike { frame, scale, fine, cargo })`: the household
     layer's fine bike (tyre rings, spokes), a rack's coarse one (solid
     wheels), a child's (scale 0.58), a cargo bike (box in front). Used by
     solarpunk racks and the cargo-bike depot, the bike shop, glass-campus
     racks and the yards.
   - `tree(g, w, y, TreeKind, h, canopy, bark, leaf)`: Round, Conifer, Palm,
     Dead, Ball (the dome and capsule looks' clipped tree). `climate_tree`
     picks the kind by climate; parks, grounds, vacant lots, megablock
     terraces and the site kit (`parts/site.rs tree_kind/tree_world`) all
     go through it. `tub_tree`: a small tree in a timber tub (solarpunk
     balconies).
   - `planter(g, w, y, Planter::{Tub, Trough, Pot}, r, tub, leaf)`: the
     dome look's round tub, megablock troughs, the yards' flower pots.
   - `bench(.., broken)`, `bin`: the site kit's `bench_world`/`bin_world`
     and run-down parks' broken benches.
   - `garment`, `CLOTH`, `washing_run`: the washing (only the household
     layer hangs it).

   **A prop keeps its own band**: a bench, a bin and a bike always go to
   `Near`, whoever draws them and whatever layer the caller is in; trees
   stay in the base mesh; planters go where the caller puts them.

4. **Who owns what.** Architecture-bound fixtures stay in the building
   styles: sills, AC units, awnings, fire escapes, outhouses, the posts and
   wires of washing lines, the laundry poles out of windows. Anything that
   says who lives there or what the weather is comes from the household
   layer (`nd_sim::yards` decides, `nd_procgen::yards` places and draws,
   `render/yards.rs` shows): washing, toys, bikes by the door, plants,
   grills, workbenches, trampolines, surfboards.

   A style gives only its flavour: where it draws a line or a pole it
   leaves a `WashLine` (`Geometry::washing`: two world points, the sag,
   window pole or not). The chunk job keeps them with the home's free
   ground (`Spots::lines`). When a household there has washing out (dry
   daytime weather: `nd_sim::yards::laundry_out`), it hangs runs of washing
   on a line of the kind it wants (a flat a window pole, a house a line;
   else the other kind) before it would put out its own airer, line or
   pole. So washing is never out in the rain or at night, and never shows
   twice. Styles that leave lines: brick-row back yards, garden lawns,
   lantern alleys, laundry poles (dense, lantern, megablock, towers),
   patchwork lines, stacks' roof wires, walk-up roof lines.

## The passes, one by one

| pass | code | draws | band | how it is batched |
|---|---|---|---|---|
| Building detail | `nd_procgen::buildings::*` into `Lod::Mid/Near`; `render/chunk_jobs.rs` | the styles' fixtures, racks, benches, bins | Mid / Near, per detail cell | merged per chunk detail cell and surface |
| Grounds of odd lots | `buildings/grounds.rs` (in the building's chunk) | garden paths, trees, promenade rails, benches | rails Mid, benches Near, trees base | as building detail |
| Household layer | `nd_sim::yards`, `nd_procgen::yards`, `render/yards.rs` | a household's things and its washing on the style's lines | Close | one mesh per look, an entity per thing: Bevy batches each look |
| Parks | `buildings/park.rs` via `render/lots.rs sync_parks` | lawns, paths, features, trees; benches, bins and litter | benches, bins, litter Near; the rest base | one set of meshes per park |
| Vacant lots | `nd_procgen::vacant` via `render/lots.rs sync_vacant` | dirt, weeds, fences, tagged walls; rubbish, tyres, scrap, the parklet bench and bin | junk, bench, bin Near; the rest base | merged per chunk |
| Murals and gardens | `nd_procgen::social`, `render/social.rs` | murals (base), garden beds (base), plants and water butt | plants Near | merged per 4-chunk cell |
| Undercroft | `nd_procgen::undercroft`, `render/undercroft.rs` | market bays under the decks | their Mid/Near layers | one set |
| Skid row | `nd_procgen::society`, `render/society.rs` | camps, RVs, trash | base (see todo) | merged per cell |
| Street look | `render/street_look.rs` | real lamp lights (nearest the view, 320 m reach), ground light pools, wet roads | lights: own pool by count, not a detail band | |
| Worn paths | `render/foot_wear.rs` | worn-ground masks on soft ground and paving | none (a texture) | a sheet per tile |

## Measuring

`status.render` carries `detail_triangles` / `detail_in_range` (building
detail generated and within its band now), `visible_meshes` and
`visible_detail_meshes` (mesh entities the camera drew last frame, before
Bevy's batching, and those under a band) and `yards` (household things
placed). `yards` (remote) gives the household layer's reach, fade and eye
limit.

Before/after numbers for the unification are in the r13-detail report
(todo.md, "Unify the close-up detail passes").
