# District 2040 (working title Neon District)

A procedural near-future city builder built to be tinkered with: worn, realistic 2040 rather than neon. Rust, Bevy 0.19, egui.
The simulation is engine-independent and deterministic; the renderer is a thin layer on top.

```
cargo run --release -p nd_app          # the game
cargo run --example headless -p nd_sim # simulate a city in the terminal
cargo test                             # sim tests
```

First build takes several minutes (Bevy). After that, `cargo run -p nd_app --features fast-dev`
links Bevy dynamically for quicker iteration.

## Playing

- Everything you draw goes into a **project**. `Ctrl+Enter` implements it, `Ctrl+Z` / `Ctrl+Y`
  undo and redo, Discard throws it away. Planned roads show as holograms, removals in red.
- **Road tool [2]**: pick a preset (Lane, Street, Avenue, One-way, Ramp, Highway) or set lanes per
  direction, choose Straight or Curved (`C`), click points, Enter or right-click to add. Ends dropped
  near any road snap onto it (white ring) and split it into a junction; curved roads bend smoothly
  through every point. A one-way Ramp joining a road becomes a yield merge with no signals. Roads snap
  to existing nodes and split anything they cross.
- **Zone tool [3]**: pick a land use, click polygon corners around a road, Enter to add. Lots are
  carved along every road inside the zone. Ctrl-click inside a zone to repaint it; a zone drawn
  over an older one takes the ground.
- **Edit [4]**: click a road to select it and change its lanes, preset, direction (flip) or curve in
  the panel; buildings along it stay. Drag control points of any road or zone, shift-click an edge to insert a point,
  Delete to remove the hovered point. Changing a built road rebuilds it on implement.
- **Bulldoze [5]**: click a road or zone to mark it for removal.
- **Inspect [1]**: click a glowing citizen, a car, or a building. Tick *Spotlight* to route a
  citizen through the LLM mind (a mock backend today).
- Traffic: cars follow lanes, queue, and obey the signals at junctions with three or more roads.
- Space pauses, `-`/`=` change speed. Camera: right-drag rotate, middle-drag or shift-drag pan,
  wheel zoom, WASD/QE. Tools by digit `1`–`8` (letters `I R Z B N T Y` also work), `O` toggles
  zoning outlines, `V` land value, `P` the pedestrian network, `G` seeds a demo district, `F5`–`F12`
  jump the camera (busiest junction, tiers, parks, plants, stations), Backspace drops the last
  point or stop.
- Every shop, plant and venue is a business with stock, cash and prices. Grocers need produce, flour,
  meat and dairy; mills need grain; assembly plants need parts. Businesses buy from the cheapest
  supplier (trucks drive the goods over) or import at a markup. Citizens get hungry, shop, and
  leave if they cannot eat. Businesses that cannot pay wages close and go derelict.
- Buildings appear on vacant lots when demand is high, in the tier the land value supports. People
  move in when they can afford a home, take jobs, commute, and move or leave when rent bites.
  `V` toggles the land value overlay; the panel shows class counts, rents and the Gini index.
  `F7`/`F8`/`F9` jump the camera to a random low, middle or high tier building; `[` and `]` zoom.
- **Paths**: the Path preset draws a 3 m way for walkers and bikes only. Cars never route over it, it
  joins streets at junctions without signals, and where it crosses a street mid-block it gets a zebra.
  Homes may line a path (shops and industry may not); residents of such homes leave the car at the
  nearest street and park in a lot nearby.
- **Getting around**: everyone walks at human pace, bike owners cycle trips over 40 m on the road
  shoulder or down paths, car owners drive trips over 60 m. Ownership follows class: most poor
  citizens have a bike and no car, the well-off have both and take bike rides to parks. Figures
  move smoothly between ticks; one sim minute shows 1.5 seconds of motion, the same scale as traffic.
- **Walking**: people route over a pedestrian network that runs along pavements, down paths, around
  the setbacks of every lot, through the alleys of the Stacks and through gates between neighbouring
  lots, so they cut through blocks. Shops count the feet that pass and sell to walk-ins; `P` shows
  the network, the inspector shows a lot's foot traffic.
- **Parking**: drivers need a space. Towers have their own, middle blocks a few, cheap buildings none.
  When drivers keep circling, a parking lot opens nearby (surface lot on cheap land, multi-storey car
  park in the middle) and charges per visit; its bays fill visibly as spaces are taken. The City
  panel shows spaces, lots, drivers circling and fees.
- **Parks**: park zones may line footpaths. Every park parcel that touches another is one park:
  paths run from lot to lot, features are spread over the lots by area, and every lot gets benches.
  Cheap land gets a bare lot with a court; the middle gets playgrounds, a pond and a kiosk; dear land
  gets a lake, a fountain, a bandstand, a carousel and a ferris wheel. Park paths are part of the
  walking network, so people cut through parks, and a visit ends on a bench, sitting, in the open.
- **Rail [8]**: a trunk line already crosses the map to the south. Lay branches with the Road tool's
  Rail preset (track snaps only to track, forks leave tangentially, corners are rounded; crossing a
  street makes a level crossing). Stations and freight yards are pieces with track along one side and
  a street along the other: drop them anywhere, even in open country. Beside track or a street they
  fuse to it; otherwise join track and road to the stubs they lay out. Nothing runs until both sides
  are connected. Passenger trains call every two hours and people ride between stations
  for long trips; freight trains bring imports to the yard at a discount and lorries take them on.
  `F5` jumps the camera to a station or yard.
- **Buses [7]**: draw a line by clicking streets round a loop (the side you click is the kerb the bus
  serves); Enter closes it. Click a stop to select its line, drag to move, Delete to remove. The panel
  sets buses per line. Place a depot (Depot mode) on a vacant lot by a street: no depot, no buses.
  Buses leave at 05:30, dwell at every stop, and go home at 23:30. People without a bike ride when a
  line serves both ends of a long trip; they wait at the stop, pay a fare, and walk the last leg.
- **Power**: buildings need electricity. The Power tool (`6`) places a coal plant, solar farm, wind
  farm or nuclear plant on a vacant lot, paid from the treasury. Coal is cheap and steady but hangs a
  toxic haze over the district and drags land value down; solar and wind are clean but follow the sun,
  cloud and wind; nuclear is enormous and expensive. The regional grid sells a little power to cover
  gaps; beyond that the rich are served first and cheap districts go dark: windows out, shops
  producing less, residents complaining. The City panel shows supply, demand, weather and treasury;
  the inspector flags UNDERPOWERED buildings. `F11` jumps the camera to a plant, `F12` to a park.
- `Tab` walks the street: first-person on the pavement nearest the camera target, mouse to look,
  WASD to walk, Shift to run, Tab or Esc to return. Tools are off while walking.
- Each land use has its own architecture per tier: junkyards and rusted sheds on cheap industrial
  land, datacentres and container depots on dear land; shophouses to twist towers; shack bars to
  casinos. Parcels follow the roads, so corner and diagonal lots get odd-shaped buildings. The
  inspector names a building's style.
  Save and Load use `saves/city.ron`.

## Tinkering

- `assets/palette.ron`: every world colour. Glow colours above 1.0 bloom.
- `assets/rules/buildings.ron`: the building grammar per land use (floor height, windows, signs) and
  the `styles` table: which styles each (land use, tier) draws from and how often. Delete the file
  to regenerate it after an update.
  Both files hot-reload while the game runs and are created with defaults on first launch.
- `docs/DESIGN.md` is the living design doc, `docs/IDEAS.md` the backlog.

## Layout

| Crate | What |
| --- | --- |
| `nd_sim` | roads, zoning, lots, buildings, citizens, economy, clock, save/load, the `MindProvider` trait |
| `nd_procgen` | terrain, parcels' buildings (35 styles, parts toolkit), streets, seamless textures, palette |
| `nd_minds` | `RuleBasedMind`, `HybridMind` + `MindBackend` trait (`MockBackend` now, LLM backends next) |
| `nd_app` | Bevy front end: rendering, camera, tools, egui UI, hot reload |
