# Style reference: building detail from the inspiration renders

Harvey's 38 reference renders sit in `Inspirations/` (`Hong_Kong/` 14, `cyberpunk/` 24).
They are a **style and detail reference only**: nothing is traced, copied or
imported. Every detail below is built by the generators in `nd_procgen`
from the building's own seed, stylised and gritty rather than cartoonish,
and drawn the same from every camera (no billboards that turn to face you,
nothing that only exists in a close-up).

Pictures are named by file. "HK" = `Inspirations/Hong_Kong/`, "CP" =
`Inspirations/cyberpunk/`.

Costs are rough triangles per building for a mid-size tower (12 floors,
four 14 m faces) and say which **detail layer** a part goes in
(`Geometry::detail`, see [Level of detail](#level-of-detail)):

- **base**: the building's mass and anything that reads from across the
  map (walls, ledges, big signs, lit windows, neon). Always drawn.
- **mid**: things that read from a few blocks away (awnings, billboard
  frames, rooftop tanks and shacks, cages, stair cores' steel). Drawn to
  about 700 m.
- **near**: the grain you only see up close (AC units, laundry, pipes,
  window frames, antennas, railings, streaks). Drawn to about 320 m.

A box is 12 triangles, a decal (flat quad) 2.

---

## 1. Hong Kong tenements

Pictures: HK `3imSouswa7CjpeQCykNUKk3A`, `DlMlDIHbjhmLWwrPNbQpoDYAQ` (the
whole family side by side), `QBo7caxxC1RTSEfsF6hb5LN13gM`,
`Qc0tMhV9pyYUg78NQWUVsfAZOw`, `UtPRN5WM4YUlIlzVx8FGxawyzUI`,
`VObctrB0gHziLcqFrTXdOzrnmbA`, `gHQhwmindtPQnM83xVpheUYCcVw` (a street of
them in sun), `iaEUqUfwsjbovukdI1Pmzy5gh4`, `jbFbvPUlxu4UAs9mVrjf8y2tXZ4`,
`kucwrrfg8yVh6KiaRGH7XZwE`, `ofNSOD0TzqfV0ZCU2I4ssOa2pQE`,
`tMnZWXLdbSICC0UucJkxvNFjI`, `tR5uA6QbQ9Tdph8IEIi2iAg7O6E`,
`wsYHoAWeOVfBecCgxXEWpWzl7Y`.

**Feeds:** Kowloon stacks look and The Walled Quarter story
(`WalledCity`, `PencilTower`; the Stacks share the street level), Lantern
Alley (the walk-ups and shophouses: the low forms), the Neon strip's
`PencilTower`.

### What recurs

| Detail | Seen in | How to build it | Layer, cost |
|---|---|---|---|
| **Tower on a podium**: a slab of 8–30 floors standing on a 1–2 storey podium that fills the lot | `3imS…`, `UtPR…`, `jbFb…`, `ofNS…`, `tR5u…` | The podium is the lot polygon extruded `podium_floors × fh`; the tower a clipped box on it (already in `pencil_tower`) | base, existing |
| **Deep ledge over the shops**: a thick concrete slab 1–2 m proud of the podium at the first floor, often with a rounded corner, shop lights under it | `QBo7…` (curved corner), `UtPR…`, `tR5u…`, `ofNS…`, `wsYH…` (red band) | A slab per street face at `y = fh`, 0.35–0.5 m thick, out to `reach_out(face)` (never past the lot by more than the sign allowance); a darker underside band; a row of small lit fittings under it at night | base slab 12 per face; mid fittings 2 each |
| **Dense window grid, a different frame per flat**: hundreds of small windows, but the frames, sills and glazing bars change from flat to flat (white aluminium, green steel, dark wood; some sliding, some open) | every HK picture; `QBo7…` and `VObc…` up close | A per-flat (per window column per floor) hash picks frame colour, frame type (plain, mullioned, hopper open) and sill; one thin frame box per window plus a mullion bar on some. Hash from `(seed, face, floor, column)` so it is stable and draws nothing from the building's stream | near, ~14 per window |
| **AC units under most windows**, on little brackets, some with drip stains below | every HK picture; densest in `kucw…`, `iaEU…`, `3imS…` | A 0.7×0.45×0.4 box on a thin bracket bar under 50–80 % of windows (by tier and wear); an occasional pair or a big split unit at the podium | near, 24 per unit |
| **Laundry poles** out of windows, washing hung on them | `3imS…`, `QBo7…`, `VObc…`, `iaEU…`, `kucw…`, `tMnZ…` | A pole along the face normal from a sill, two to four cloth quads of palette tarp colours hanging off it | near, ~40 per pole |
| **Caged balconies** (steel "birdcages") and enclosed balconies | `3imS…` (whole columns of them), `VObc…`, `gHQh…` | A steel frame box around a window bay: four corner posts, top and bottom rails, a mesh quad front (the `Matte` surface, dark); some with a little tin roof | mid, ~60 per cage |
| **Side wing with a stair or lift core**, blank but for slit windows, with **pipes and a ladder-like run** up it | `3imS…` (core on the left), `jbFb…` (pink core with a pipe ladder), `ofNS…` (steel cage stair), `UtPR…` | A narrow box (2.5–4 m) up one side of the tower, 1–2 floors taller than the slab, with a roof cap; slit windows every floor; 2–4 vertical pipes (thin boxes) with brackets every floor and horizontal branches into the slab | base core 12+; near pipes ~12 per pipe per 3 floors |
| **Mixed facades**: one building in stained pink tile next to bare concrete or grey render | `3imS…` (red-brown tile vs. grey render), `QBo7…` (white tile, blue-grey render, dark concrete), `jbFb…`, `UtPR…` | Pick the tower's and the core's surfaces and colours separately (tile reads as `Panel` with a pink or white tint, render as `Plaster`, raw as `Concrete`); per-floor colour bands on the slab between window rows | base, 12 per band |
| **Damp and rust streaks**: dark water stains from sills and AC units, rust runs under brackets, a darker band at the foot | every HK picture; heavy in `iaEU…`, `kucw…`, `VObc…` | Thin vertical decals in a dark, slightly green or rust colour hung from the bottom of windows and units, 1–4 m long, a few per face; the existing `grime` adds blotches; the weathered `Worn*` materials carry the fine grain | near, 2 per streak |
| **Rooftop clutter**: antennas (TV yagis on masts), water tanks, satellite dishes, rooftop shacks of tin and plywood, railings round the roof, a stair hut | every HK picture; `VObc…` (shacks, timber), `kucw…` (antenna forest), `tMnZ…` (stepped roof, dish), `wsYH…` | `antenna_forest` exists; add yagi crossbars on masts, a parapet railing (posts and a rail), a stair-and-lift hut on the core, a water tank on legs | mid tanks/shacks 12–40 each; near antennas and railings ~20 per mast, 4 per railing post |
| **Roller shutters** on the shops, half down, stickered | `Qc0t…`, `UtPR…`, `iaEU…`, `kucw…`, `tMnZ…` | A metal decal with horizontal slat lines (two-tone decal strips) and a darker box at the top (the drum); random height (fully down to half up) per shop | base 2 + mid ~10 per shutter |
| **Coloured awnings**: striped or plain canvas, green, blue, red, tan, on steel arms | `QBo7…`, `Qc0t…`, `UtPR…`, `iaEU…`, `kucw…`, `tR5u…`, `wsYH…` | A sloped panel from the fascia out over the pavement, a valance strip at the front edge, two thin arms; colour from `awning_colors`, stripes as alternating thin decals | mid, ~40 per awning |
| **Big billboards on the podium** facing the street, lit at night | `Qc0t…` (yellow board), `tMnZ…`, `jbFb…`, `3imS…` | A board on the ledge or podium face, a dark frame and a lit face; a few lamp arms over it | base face (glow), mid frame and arms |
| **Vertical neon signs on the corners**, standing out on steel brackets, a column of characters | `3imS…`, `iaEU…`, `kucw…`, `ofNS…`, `tR5u…`, `wsYH…` | `vertical_sign` exists; mount it on the tower corner on two or three brackets, three to six lit glyph blocks with an outline tube | base (glow stays visible at night), mid brackets |
| **Hanging signs** over the street at right angles to the facade | `UtPR…`, `tR5u…`, `wsYH…` | `hanging_sign` exists | base |
| **Street furniture** round the corner: bins, fire hydrants, traffic signals, guard rails | `Qc0t…`, `UtPR…`, `iaEU…`, `tR5u…` | Belongs to the street generator, not the building | street |

### Forms the family takes

- **Pencil tower** (`jbFb…`, `ofNS…`, `UtPR…`): narrow slab on a podium,
  a core up one side, a crown of setback floors with tanks and masts.
- **Walk-up block** (`iaEU…`, `kucw…`, `tMnZ…`, `wsYH…`): 5–7 floors over
  shops, rounded corner balconies with deep projecting slab edges each
  floor (the "ribbed" look), a stepped roof with shacks. This is the
  `WalledCity` low form.
- **Slab on stilts** (`3imS…`, `DlMl…`): one wide slab next to a taller
  narrow one, a mismatch of materials and heights. The `WalledCity`
  high form, cells of mismatched height.

---

## 2. Cyberpunk blocks

Pictures: CP `BnfchNSAglZDdDjMrbSm5nM0I`, `BsChj0sJaYTyYuvr77JOw8YqNYc`
(night street), `DjNgjK0IAkVjA7fSsMvBChuoo8g`, `Dlfwsu1mRFiiaqcvLu7AUjYOxPc`,
`F3viABh8EVov4IQ8UsdPckIkNtc`, `KRCknv4hS7D0vof9oJ6q5sG7FHk`,
`N5o3TMghwzBABUQFot6O1q2ZPZM`, `NjYdmDl6Larf92Z8zyW57iZJtY`,
`PGKp2BZxkA6n6woct88YszoKLzk`, `QgKoZVq5SoJtPiowQT1tbTKZY` (the family),
`RScHWAcPP5Pjdjd3wXJzDHlhgRQ`, `VpaN0fNJZN2g6vZnJwTit5eNe3E`,
`X2ltp2Ysw5lNYQLlam4AdNK58`, `YArqhXqN0sVOhYxR5AH14HNcwk`,
`dwbE7BRnqoPGC2XdjyNvhCpIs`, `e1bEYuXULAQlaWxuvzdWJZbq9bw`,
`hGf20cjKctEak7H0m1cwSL70bGA`, `hXXd6LaLZbJqxFSWrsgWWOXBBg`,
`ksk1K1mcAh7DwvttUXhYu3y51rI`, `tdo0a7h9X3ZcC3hVgb3JDwy6E`,
`uBYUCu0Nq3MzPmXn83D2xdn2FRU`, `vreeeG4frXLlE9qAoSotSLXHhjM`,
`wsa0Nv6tP7cSGxovNWhbWKS8X9c`, `xWvaJfYsLo5sWcCPaA4w26mWZ8w`.

**Feeds:** Neon strip (`NeonTower`), Capsule (the pods and plant, not the
domes), Skyline Luxe (slim towers with screens), Glass Campus (labs and
server halls), offices (`PodiumOffice`, `SlabTower`), industry
(`Datacenter`, `FlattedFactory`, `FabPlant`, `Depot`).

### What recurs

| Detail | Seen in | How to build it | Feeds | Layer, cost |
|---|---|---|---|---|
| **Chamfered concrete masses with panel seams**: big boxes with 45° cut corners top and bottom, horizontal seam lines every 0.6–1 m, vertical joints | `NjYd…`, `Dlfw…`, `BnfchN…`, `hXXd…`, `e1bE…` | Loft the mass with its corners cut (`offset_edges` on the vertical faces or a frustum at top and bottom); seams as thin dark decals in bands per face, every 2nd one broken | offices, industry, Neon strip | base mass; near seams 2 each |
| **Cantilevered boxes and floors stacked out of the frame**: storeys and pods hung off a core, wider than what holds them | `Dlfw…`, `F3vi…`, `hGf2…`, `hXXd…` (round pods), `tdo0…`, `xWva…` | Boxes off the faces at alternate floors with a chamfered underside; capsule pods (`capsule_hotel`) are the model | Capsule, Neon strip, offices | base |
| **Tall louvred vent panels and big fans**: whole bays of horizontal louvres, round fan units in a frame, sometimes a column of eight | `BnfchN…`, `Dlfw…`, `PGKp…`, `dwbE…` (louvre bands round the podium), `wsa0…` (a column of fans) | A recessed panel with 8–20 thin slanted slats (boxes) over a dark back; a fan is a short cylinder (`turn`) with a cross of blades decal | industry, Datacenter, offices | mid slats 12 each; near fans ~60 |
| **Exposed stair cores with diagonal bracing**: a steel frame tower up the side with X or V braces, grating landings, a zig-zag stair inside | `BnfchN…`, `PGKp…`, `RScH…`, `e1bE…`, `vree…`, `xWva…` | Four corner posts, a beam every floor, one diagonal per floor alternating, a stair slab per flight (`stair_zigzag`), mesh railings | all | mid frame ~12 per member; near rails |
| **Open steel frames with holographic text signs** running round a floor or a crown | `BnfchN…`, `KRCk…` | An open floor (posts only) with a ring of glyph decals in a single glow colour on all four sides, set just inside the posts; at night it is a ring of light | Neon strip, Skyline Luxe | base glow |
| **Lit glass storeys with screens inside** | `Dlfw…`, `ksk1…`, `uBYU…` | A band of glazing with a large lit panel set behind it (`led_screen` behind `Glass`) | Skyline Luxe, Glass Campus | base |
| **Facade-high LED screens and vertical banner signs** | `F3vi…`, `N5o3…`, `ksk1…`, `uBYU…`, `VpaN…`, `wsa0…`, `BsCh…` | `led_screen` and `vertical_sign`, sized to whole bays; a dark frame round each | Neon strip, Skyline Luxe | base glow |
| **Railings round every roof and terrace** | `Dlfw…`, `KRCk…`, `NjYd…`, `X2lt…`, `YArq…`, `hGf2…` | Posts every 1.5 m and two rails round the roof edge; the same on terraces and landings | all | near, ~4 per post |
| **Antennas, dishes, a lattice mast, rooftop plant boxes** | `Dlfw…`, `KRCk…`, `YArq…`, `vree…`, `xWva…` | `roof_gear` and `antenna_forest`; a lattice mast is four thin posts with X bracing; plant is `chiller` boxes | all | mid boxes; near masts |
| **A big lit brand sign on the facade** | `Dlfw…` ("LIFE CORP"), `N5o3…`, `ksk1…` | `brand::decorate` already hangs the owner's brand; size it to a whole bay | offices, megacorp | base |
| **Deep soffits with strip lights**: a concrete overhang over the ground floor with a grid of lit fittings under it | `X2lt…`, `dwbE…`, `tdo0…` | A slab out over the pavement with a row of small glow decals on its underside | offices, Neon strip | base slab; mid fittings |
| **Street level round the plot**: concrete barriers, bollards, bins, vending machines, a parking pylon (P P P), traffic lights, guard rails | `DjNg…`, `PGKp…`, `RScH…`, `vree…`, `xWva…`, `NjYd…` | Vending machines are lit boxes against the wall (glow fronts); barriers a run of low boxes; the pylon a tall lit sign by the kerb | Neon strip, offices | mid, ~30 per item |
| **Roller shutters and loading doors** with a lamp over each | `DjNg…`, `PGKp…`, `X2lt…`, `dwbE…` | `roller_door` exists; add slat lines | industry, offices | base + mid |
| **Grime**: dark drips down from every ledge, edge chips, rust on the steel | every CP picture | `grime`, the weathered materials, and drip decals under ledges (as the HK streaks) | all | near |

### Forms the family takes

- **Corner block** (`Dlfw…`, `NjYd…`, `YArq…`): 5–7 floors, chamfered
  mass, vent panels up two corners, a screen, roof railings.
- **Slim tower with an external frame** (`F3vi…`, `VpaN…`, `uBYU…`,
  `e1bE…`, `vree…`): a narrow tower with a braced steel frame or lift run
  up one side and a screen the height of the facade.
- **Mega-block** (`wsa0…`, `N5o3…`): a slab with a hole through it, fan
  walls, two screen panels, bridges across the void.
- **Frame building** (`KRCk…`, `BnfchN…`): an open steel frame holding
  boxes, a holographic text ring, a cantilevered pod block.

---

## Level of detail

These details repeat thousands of times, so they are built for a big city:

- Every detail is part of the merged per-chunk meshes (one mesh per
  surface per 128 m chunk, as the buildings already are), never one entity
  per AC unit.
- `Geometry` carries two extra detail layers (`Geometry::detail`, `Lod::Mid`
  and `Lod::Near`). A generator opens a layer with `g.lod(Lod::Near, |g| ...)`
  and everything it draws inside goes there; glowing surfaces always stay in
  the base mesh so the night city reads the same from afar.
- The renderer spawns each chunk's detail layers as their own meshes with
  a `VisibilityRange` (Bevy's HLOD): near detail drops out past about
  320 m from the camera, mid detail past about 700 m. The shadow passes
  use the same ranges, so detail costs nothing in a far view.
- The detail pass draws from its own random stream
  (`rng_for(seed, DETAIL_STREAM)`), so the building's base mesh is the same
  as before the detail existed and the details are the same every time the
  building is drawn, from every camera.
- Per-window choices (frame type, AC, streak) come from a hash of the
  building seed, the face, the floor and the column, not from the stream,
  so adding a floor does not reshuffle every flat above it.

Rough budget for a 12-floor HK tower with four 14 m faces (about 300
windows): base +150 triangles (ledge, core, bands, shutters), mid +1,500
(awnings, cages, tanks, shacks, brackets), near +9,000 (frames, AC units,
laundry, pipes, railings, streaks, antennas). Drawn near the camera only,
a view down one street shows a few dozen such towers.

---

## Status

- Done (r4-style): this guide; the Hong Kong pass on `WalledCity` and
  `PencilTower` (podium ledge, per-flat frames, AC units, laundry, cages,
  side core with pipes, mixed facades, streaks, rooftop clutter, shutters,
  awnings, podium billboard, corner neon); the detail layers and their
  distance ranges.
- Done (r4-lantern): **Lantern Alley** (`buildings/lantern.rs`, the
  Lantern Lanes story's look, the low forms of section 1): shophouses
  with an arcade, alley walk-ups, stall rows, a food court hall, a tea
  house, a night-market hall behind a gateway, workshops, back-alley
  kitchens and a courtyard compound for the story's mansions. Paper
  lanterns: the paper body is mid detail (12 triangles, cap and tassel
  near), the light a base-mesh glow box just round it marked
  `NIGHT_MARK` (12 triangles, lit after dark only), strung across what a
  lot holds (its alley, court, aisles, arcade). Uses the HK pass's
  `flat_windows`, `ledge`, `awning`, `roof_clutter`, `pipe_run`.
- Done (r4-garden): the Garden Suburb look (Quiet Acres; `buildings/garden.rs`):
  houses, bungalows, a close, a parade, a garden centre, a corner café and the
  garden manor, with fences' pickets, frames, glazing bars, gutters, aerials,
  bins, beds and garden furniture in the near layer, sheds, cars, rails and
  awnings in the mid layer; porch lanterns lit only after dark.
- Done (r4-harbour): **Harbour Pastel** (`buildings/harbour.rs`, The
  Promenade's look): pastel terraces with gables to the street, converted
  boathouses, a block with a lighthouse tower on its corner, a boardwalk
  café, a seafood shack, a striped-awning arcade, a boutique hotel with
  flags, the fish market hall, net sheds, and a pastel villa with its
  boathouse for the story's mansions. Salt-faded pastels (greyer and
  darker with wear), white trim yellowing, tar-black sheds. String bulbs:
  wire mid detail, the bulb's glass near detail, its light a base-mesh
  glow box marked `NIGHT_MARK` (12 triangles, lit after dark only), strung
  along each front; the lighthouse lamp rooms glow the same way. Frames,
  sills, shutters, glazing bars, railings, chairs and pots are detail. On a
  lot that touches the water (`Ctx::shore`) a small pier on piles runs out
  over it, unless the business builds its own jetty.
- Done (r4-solarpunk): **Solarpunk Green** (`buildings/solarpunk.rs`, Eco
  Village's look): timber-and-glass terraces under planted roofs (flat
  behind an upstand or a sedum mono-pitch), solar-shingle townhouses
  (steep mono-pitch roofs skinned in PV to the street), a vine-covered
  tower with balcony gardens, a glass market hall with a farm on the roof
  behind, a co-op store, a repair café in an old workshop, a vertical farm,
  greenhouse rows, a cargo-bike depot, and a planted solar villa with a
  roof garden for the story's mansions. Lived in, not a brochure: timber
  silvers and darkens with wear, copper patinates, glass is dirty, panels
  are patched with odd ones (a tarp over a leak on a worn roof), cables
  run down to battery boxes, rain tanks at every corner, roofs overgrown.
  The PV skin is one quad per slope in the base; patches mid, seams near.
  Creepers are decals at two depths of their own (0.011, 0.015: under the
  grime and the frames), a third of them base so a planted wall reads green
  from far off. Wind trees: trunk base, branches and blades mid. Glow, all
  base and marked `NIGHT_MARK`: the amber strip and underside of every
  solar canopy, string bulbs (Harbour's), the vertical farm's magenta grow
  lights floor by floor, greenhouses' sodium lamps, post lamps and the
  depot's charge lights. Bikes and cargo bikes are near detail.
- Left for later: the cyberpunk family (section 2) on the Neon strip,
  offices and industry; the street-level props round the plot; the other
  looks (Skyline Luxe, Glass Campus).
