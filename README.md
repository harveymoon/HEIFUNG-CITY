# Neon District

A procedural cyberpunk city builder built to be tinkered with. Rust, Bevy 0.19, egui.
The simulation is engine-independent and deterministic; the renderer is a thin layer on top.

```
cargo run --release -p nd_app          # the game
cargo run --example headless -p nd_sim # simulate a city in the terminal
cargo test                             # sim tests
```

First build takes several minutes (Bevy). After that, `cargo run -p nd_app --features fast-dev`
links Bevy dynamically for quicker iteration.

## Playing

- **Road tool [2]**: click nodes, Enter or right-click to build. Roads snap to existing nodes and
  split anything they cross.
- **Zone tool [3]**: pick a land use, click polygon corners around a road, Enter to zone. Lots are
  carved along every road inside the zone.
- **Inspect [1]**: click a glowing citizen to read their personality, memories and current thought.
  Tick *Spotlight* to route them through the LLM mind (a mock backend today).
- Space pauses, `-`/`=` change speed. Camera: right-drag rotate, middle-drag or shift-drag pan,
  wheel zoom, WASD/QE.
- Buildings appear on vacant lots when demand is high. People move in when homes are free, take
  jobs, and commute. Save and Load use `saves/city.ron`.

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
