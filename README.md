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
- **Road tool [2]**: pick lanes per direction, click nodes, Enter or right-click to add. Roads snap
  to existing nodes and split anything they cross.
- **Zone tool [3]**: pick a land use, click polygon corners around a road, Enter to add. Lots are
  carved along every road inside the zone. Ctrl-click inside a zone to repaint it; a zone drawn
  over an older one takes the ground.
- **Edit [4]**: drag control points of any road or zone, shift-click an edge to insert a point,
  Delete to remove the hovered point. Changing a built road rebuilds it on implement.
- **Bulldoze [5]**: click a road or zone to mark it for removal.
- **Inspect [1]**: click a glowing citizen, a car, or a building. Tick *Spotlight* to route a
  citizen through the LLM mind (a mock backend today).
- Traffic: cars follow lanes, queue, and obey the signals at junctions with three or more roads.
- Space pauses, `-`/`=` change speed. Camera: right-drag rotate, middle-drag or shift-drag pan,
  wheel zoom, WASD/QE.
- Every shop, plant and venue is a business with stock, cash and prices. Grocers need produce, flour,
  meat and dairy; mills need grain; assembly plants need parts. Businesses buy from the cheapest
  supplier (trucks drive the goods over) or import at a markup. Citizens get hungry, shop, and
  leave if they cannot eat. Businesses that cannot pay wages close and go derelict.
- Buildings appear on vacant lots when demand is high, in the tier the land value supports. People
  move in when they can afford a home, take jobs, commute, and move or leave when rent bites.
  `V` toggles the land value overlay; the panel shows class counts, rents and the Gini index.
  `F7`/`F8`/`F9` jump the camera to a random low, middle or high tier building; `[` and `]` zoom.
  Save and Load use `saves/city.ron`.

## Tinkering

- `assets/palette.ron`: every world colour. Glow colours above 1.0 bloom.
- `assets/rules/buildings.ron`: the building grammar per land use (floor height, windows, signs).
  Both files hot-reload while the game runs and are created with defaults on first launch.
- `docs/DESIGN.md` is the living design doc, `docs/IDEAS.md` the backlog.

## Layout

| Crate | What |
| --- | --- |
| `nd_sim` | roads, zoning, lots, buildings, citizens, economy, clock, save/load, the `MindProvider` trait |
| `nd_procgen` | terrain, building geometry, palette |
| `nd_minds` | `RuleBasedMind`, `HybridMind` + `MindBackend` trait (`MockBackend` now, LLM backends next) |
| `nd_app` | Bevy front end: rendering, camera, tools, egui UI, hot reload |
